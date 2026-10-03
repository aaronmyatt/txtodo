# first-sync-speed

## Goal

A new device replays every op from #1 to sync. That part stays. What we fix is the cost of
replaying them. No wire change, no ADR.

## How it works today (2026-10-03 trace)

- Greet/Want with per-origin heads. An empty device wants `1..=head` for every origin
  (`txtodo-sync/src/want.rs:11`).
- The sender serves one origin's whole run before the next (`lan_session_live.rs:314`). Any op
  that builds on another device's task parks until that device's run arrives.
- Batches hold up to 1000 ops, with 2 in flight (`WINDOW_BATCHES`). A refused batch waits
  `RESEND_AFTER = 10s` (`lan_session_live.rs:52-61`).
- The receiver does one actor commit per same-file run (`lan_apply.rs:296`). Each commit clones
  the DocState and the parked queue, renders the whole file, upserts every task's fingerprint, and
  fsyncs plus renames the file (`sync_ops.rs:48-67`, `actor_mirror.rs:31`).
- `Parked::retry` rescans the whole queue (up to 10k) after every group that lands
  (`sync_park.rs:58-85`). Past `MAX_PARKED_OPS` the oldest op is skipped for good, so ops are lost.
- `DocState::apply` scans every slot, ghosts included, for each op (`state_ghosts.rs:96-135`).
  `partition-converge/notes.md:214` already names the fix: an id to slot index.
- The next open replays each file's log from empty (`log_repair.rs:106`), which pays the same
  costs again.

## Design

- Measure first. `lan_sync_bench.rs` covers one origin and a small file, so it misses all of the
  costs above. The new bench needs several origins, moves, notes.md and sub-lists.
- Interleave origins: the sender picks the next batch by lowest HLC across `runs`. Each batch
  still holds one origin's run, so the wire stays the same. Open question: does the receiver's
  head gate (163ca58c) accept the interleaving as is? It is per origin, so it should.
- The index, the targeted retry and the per-batch commit are local changes.
- Open from a snapshot: the local `snapshots` table already exists (`store/src/projections.rs:93`).
  Check that log repair still forces a replay from empty.

## Open questions

- The chaos order bug (root line, 20261002-232638) also touches group parking. Do it first, or
  keep it in mind, so the retry rework does not hide it.

## Baseline (2026-10-03, debug build)

Bench: `first_sync_bench_tests.rs`, `slow_first_sync_bench` (in-crate, `--profile ci`). Three
devices make 40 edits each per round (adds, edits, `do`, reorders, 4 sub-lists, notes.md) and
sync in HLC order after each round. Then a fresh device pulls it all over the real
`drive_shared_session`; two more take it straight through `commit_incoming_ops`, one in today's
per-origin order and one merged by HLC; then the synced device reopens.

20 rounds, 2743 ops, 5 lists:

| phase | ms |
| --- | --- |
| session (real link, sender's order) | 9 106 (1 232 commits, ~2.2 ops each) |
| direct, per-origin order | 8 980 |
| direct, HLC order | 12 900 (more same-file runs, so more commits) |
| reopen | 695 |

Debug build only; release not measured yet. `RESEND_AFTER` is 300 ms under test and nothing was
refused, so the 10 s resend cost does not show here.

Where the time goes (macOS `sample`, 15 s over the sync phases, receiver side):
- `FileActor::commit` is ~85% of `on_sync_ops`.
  - ~40%: writing the list file with fsync (`F_FULLFSYNC`), once per commit.
  - ~37%: `duplicates::duplicate_groups`, called from `stored_change` on every commit to fill
    `Change::duplicate_groups`; it parses every line (`strip_own_id`) each time. Not in the plan
    above; a new line below.
- `apply_parking` plus `Parked::retry`: ~13%. `has_placement` is the only slot scan that shows.
- Sender: `serve_want` is mostly per-op ed25519 signing (debug curve math).

So the per-batch commit is the big one; the slot index and targeted retry are small at this size.

### Found: per-origin order loses text edits

With today's sender order the fresh device (session and direct per-origin, and so its reopen)
ends with lines missing their last edit: A has `task 24 … +e342 +e852`, the new device
`task 24 … +e342`, in all 4 lists. HLC order converges. Likely an `EditText` that builds on
another origin's edit lands first, does not fit, and is skipped for good: the notes.md case that
9efd4c74 fixed, but for todo.txt. The bench now prints every file that differs and fails on it.

## As built

### Sender interleave (2026-10-03)

- `lan_serve_merged.rs`: `Live::push` takes one batch from the owed runs merged by HLC (lazy
  128-op reads per origin), not the first origin's run. The batch ends where an origin would come
  back, so each origin appears once: a 0.0.20 receiver checks every run against its store head
  before the commit (`sync_commit_gate.rs`) and would refuse a second run of the same origin.
  `file_carrier.rs` still uses `serve_want` (per-origin).
- Bench, 20 rounds, debug: the session device now converges (it lost edits before). Reopen
  695 → 117 ms: its log is in HLC order now, so the replay at open parks nothing.
- Known cost: session 9.1 → 14.6 s. HLC order cuts a batch into more same-file runs, so more
  commits (the same gap as direct_hlc vs direct_origin), and batches are smaller (an origin's
  burst, not 1 000 ops), so more acks. The per-batch commit line is what pays this back.
- Devices that already hold a log in per-origin order keep it; their replay at open still parks.

### Slot index: dropped (2026-10-03)

The slot scans are ~4% of a sync commit in the bench (`has_placement`; `live_slot` and
`anchor_slot` under 1%). Slots shift on every insert, so an id-to-slot index needs a rebuild per
op, as `reindex` already does: a constant factor at best. Revisit if a 10k-line bench says so.

### Parking: targeted retry, text edits wait (2026-10-03)

- `sync_park.rs`: waiting ops are kept by arrival number with an index from each task they name.
  A group that lands retries only the groups naming a task it touched (inserted, edited, moved),
  in the same round order the whole-queue rescan had; a failed op keeps its number, so the queue
  order is as before.
- An `EditText` that does not fit (`StateError::Text`) now waits on its task like a missing one,
  and lands once an edit of that line does. This was the bench's lost-edits bug; all four phases
  converge now, per-origin order included, so logs already stored in that order replay right too.
- Changed on purpose: an op that fails on retry with an error that cannot wait is skipped there
  (logged `sync_op_skipped`); it used to stay in the queue and be retried after every landing.
- Past `MAX_PARKED_OPS` the oldest is still skipped, now with its own warn,
  `sync_parked_overflow`. Not fixed: it is lost until something replays it, and a replay of the
  same order overflows the same way. With HLC-ordered batches a queue that long needs a peer whose
  log lacks the op it waits for.
- Bench, 20 rounds, debug: session 15.3 s, direct per-origin 8.8 s, direct HLC 13.1 s, reopen
  114 ms. The queue is short in this history, so the targeted retry does not show in time here.

### One commit per batch: needs a human call (2026-10-03)

The fsync per commit is ~40% of a sync commit, so fewer commits is the big win, but the obvious
way is not safe:
- A batch interleaves files inside one origin's run (X, Y, X). Grouping its ops by file inserts
  that origin's ops out of order. The store takes a peer's numbers as given (ADR 0039), so the
  numbering survives, but heads are `MAX(origin_seq)`: if file X's commit then fails (disk, or
  the `blocked/todo.txt` mkdir case `lan_session_resend_tests` covers), Y's later ops already
  landed past X's missing ones, the ack and heads skip them, and nobody sends them again. Today
  that case stops the batch at X and the sender retries: stuck, not lost.
- Safe: one store transaction per batch across every file it touches (all ops plus each file's
  projection and `prev_hash`), then the file writes. That needs a multi-file commit in
  `txtodo-store` and a prepare/commit turn between `FileActor`s, which changes the
  one-writer-per-file commit path the daemon's invariants rest on. ADR-sized.
- Smaller: coalesce the file writes in a sync burst. Commit to the store per run as now, but
  write the file only when no more peer ops for it are queued. Needs `prev_hash` to mean "the
  bytes last written", not "the previous projection", or `recover` reads the file as a foreign
  edit. Touches crash recovery.
- Smaller still: plain `fsync` instead of `F_FULLFSYNC` for sync commits. A durability call.

Open question for a human: which of these, if any. Until then the bench pays one fsync per
same-file run, and HLC order makes more runs than per-origin order (13.1 vs 8.8 s, debug).

### Resend at once: dropped (2026-10-03)

A receiver could ask again with a `Want` from its head, and a sender could take a mid-session
`Want` as "rewind to here" (an old sender only raises `asked`, so it is compatible). But no
refusal is helped by asking at once:
- Gate refusal (`sync_commit_gate.rs`): the ops before the batch are in another session's import.
  Asked again now, it is refused again; it has to wait for that commit.
- A run that fails in `commit_incoming_ops` (mkdir, disk): the cause stays. Asking at once loops;
  `RESEND_AFTER` is the backoff, and `stuck_sync.rs` books it.
- Out of step with nothing held: only happens after one of the two above.
The bench refuses nothing, so there is nothing to measure either.

Seen while reading, not changed: after a gate refusal `commit_and_ack` returns before
`Session::committed`, so the session stays `Importing` and the next batch (`WINDOW_BATCHES` = 2
keeps one in flight) is "unexpected Ops" and ends the connection; the reconnect resyncs. Ending the
import with `committed(&[])` would keep the link up and leave it to `RESEND_AFTER`. I could not
build a case that fires the gate (a run that passes the session's heads check starts at or before
the store's head), so no test and no change.

### Replay from a snapshot: needs a human call (2026-10-03)

`repair_log` replays from empty on purpose: it checks that the log alone, which is all a peer
gets, rebuilds the file. The `snapshots` table holds bytes only. A replay started from them has no
ghosts, stamps, parents, field stamps or text history for the snapshot's lines, and ADR 0033/0034
merges need those; `adopt_stamps` is also how ghosts come back at open. So "replay from the
latest snapshot" means persisting a `DocState` checkpoint with its merge metadata, taken from a
replay of the log, and replaying only what follows it. That is a new store table and a persisted
format: ADR-sized.

How much it would buy: reopen in the bench is 114 ms (debug, 2 743 ops, 5 lists) now that the log
is in HLC order; it was 695 ms when the log was in per-origin order. Devices that already hold a
per-origin log still pay the parking in that replay.

Open question for a human: build the checkpoint, or keep replay from empty.

### Duplicate count per commit (2026-10-03)

- `commit.rs`: `Change::duplicate_groups` is counted only when the actor has a Watch subscriber;
  `watch_forward.rs` is its only reader. With none it is 0, and a client that subscribes later
  lists conflicts itself. Known gap: a subscriber that joins between the count and the broadcast
  gets 0 for that one change.
- `id_strip.rs`: `without_own_id` cuts the own `id:` word out by bytes where the cut is plain (the
  usual `… id:X` at the end of a line) and falls back to `strip_own_id`'s parse otherwise
  (`remove_tag` works inside the description, so a tab beside the word or a line that is all
  prefix differ). A proptest pins it to `strip_own_id`, and it found that case on the first run.
- Bench, 20 rounds, debug, no subscriber: session 15.3 → 11.7 s, direct per-origin 8.8 → 6.9 s,
  direct HLC 13.1 → 10.3 s. The byte cut (subscriber case) is not timed by the bench.
