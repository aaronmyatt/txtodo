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
