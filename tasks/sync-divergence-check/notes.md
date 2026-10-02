# sync-divergence-check

## Goal

Two paired devices that hold the same ops but render different bytes find out and say so. Today
nothing compares devices at run time, so a split stays silent until someone edits that line.

This is the owner's call of 2026-10-02 ("go with A"): stop chasing every rare arrival order the
lab finds (same line, same offline gap, clashing actions: `do` vs priority, reopen vs priority,
two splices, which blank a remove takes, a re-insert). Build one check that catches all of them.
The likely cases (3+ devices, editor saves, notes.md appends, plain bugs) still get fixed one by
one; this check is how we hear about the rest.

## Design (draft; the wire part is the `Decide:` line)

- **Compare only when the op sets match.** Different bytes are normal while a peer lags. So each
  side sends, per file, two hashes: one of the document's op set, one of its rendered bytes. A
  split is: op-set hashes equal, byte hashes differ.
- **Op-set hash: order-free.** XOR (or a sum mod 2^256) of blake3 of each op id in the document,
  so arrival order does not change it. Updated as ops land, rebuilt by the replay at open.
  Workspace `Heads` (txtodo-sync `message.rs:59`) is not enough: it is per workspace, and an op's
  rank can shift (`lan_apply.rs:239-243`), so equal heads is not strictly equal op sets.
- **Byte hash: already there.** `actor::hash_of` (blake3 of the projection, `actor.rs:28`), kept
  as `FileActor.hash` and `NotesActor.hash`.
- **When: a live session goes quiet.** Nothing in flight either way (`Live::in_flight`,
  `lan_session_live.rs:192`) and nothing parked (`sync_park.rs`). Parked ops are a known,
  temporary difference; do not compare while any wait.
- **Wire: one appended `Message` variant**, e.g. `Digest { workspace, files: [(path, ops_hash,
  bytes_hash)] }`. Variants are append-only and need no version bump (`message.rs:3-8, 23-26`).
  Open: what an old peer does with a variant it cannot decode. If it drops the link, send `Digest`
  only to a peer whose `Hello.protocol` says it knows it, which then is a bump after all.
- **Report, not heal.** A split is booked per peer, per workspace, per file, in memory beside
  `stuck_sync.rs`. `SyncStatusResponse.Peer` gains it next to `stuck` (`txtodo.proto:808-841`);
  `doctor_sync.rs` shows a row and doctor exits 1. Cleared when a later digest agrees.
- **Out of scope:** healing (which side's bytes win is its own ADR-level call, later); the file
  carrier (no peer to compare with).

## For the Decide line: what an old peer does with `Digest` (checked 2026-10-02)

- It ends the connection. An unknown trailing variant is a postcard `Codec` error
  (`message.rs` decode), `open_and_decode_logged` returns `Err`, `dispatch_workspace_frame`
  (`lan_session_dispatch.rs:132`) returns `None`, and `None` ends the connection (`dispatch_frame`
  doc). The peer redials, the next quiet period sends another `Digest`: the link would flap.
- There is no capability signal to gate on. `Hello.protocol` must equal `PROTOCOL_VERSION`
  exactly (`session.rs:218`, `ProtocolMismatch`), and so must every frame's version
  (`frame.rs:178`). `Hello` and `Greet` field layouts are frozen. So "send only to a peer that
  knows it" needs a version bump after all, as the draft guessed.
- Two ways:
  - A: bump `PROTOCOL_VERSION` 2 → 3 with `Digest`. Mixed old/new devices stop syncing until both
    upgrade; doctor already shows `protocol_mismatch` for that peer. One release, no flapping.
  - B: two releases. First, an unknown `Message` variant is logged and skipped instead of ending
    the connection. Later, `Digest` ships. A device still on a pre-first-release build flaps.
  - I'd take A: the devices are one person's, v0.0.x, and a clean refusal beats a flapping link.

## Rejected

- B, keep fixing each permutation the lab finds (the owner's call, 2026-10-02): each fix is days;
  the rare tier has no end, and a miss is silent.
- Let the Loro mirror decide bytes (T3 in `tasks/partition-converge/notes.md`): breaks "the mirror
  never decides bytes", an ADR-level change.

## How to check

- Unit: a debug-only seam makes one device render one file differently (never built in release).
  The check must flag that file within one quiet period, and clear once bytes agree.
- Lab: `expect_converged` also reads the split report, so a failing scenario says which file split
  with matching ops (an application bug) vs different ops (a delivery bug). Then lan-converge on
  three seeds, and chaos.
- By hand: `txtodo doctor` on two paired devices after the seam fires shows the split row.

## As built

- Line 2 (2026-10-02): `crates/txtodo-daemon/src/op_set_hash.rs`. XOR, not a sum: same
  order-freedom, and an op id is `UNIQUE` in the log (a duplicate insert errors), so nothing is
  folded in twice. Both actors build it at open from `Store::for_each_op_id_of_file` (one indexed
  `SELECT op_id`, no payload decode; fails past 50M ops rather than return a partial set), before
  `recover`/`restore_held` so their commits fold in on top; then at the one commit point each
  (`FileActor::commit_inner` after `persist_change`, `NotesActor::land` after `commit_change`).
- Parked peer ops are committed on arrival, so they count. Loro imports (`on_import`) mint local op
  ids: the two sides agree only once those ops have synced too. Fine for a check that waits for a
  quiet session.
- Bypasses: `lan_apply` `log_only` (no actor for that path) and bundle import (`store.append`
  directly) write ops without an actor; both are picked up by the rebuild at the next open. An
  actor that is open while a bundle imports into its file would be stale until reopened. Bundle
  import re-registers the workspace's documents, so this should not happen; not tested.
- Cost at open: one extra indexed read per document. Not measured on a big workspace.

