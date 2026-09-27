# notes-no-base

## Goal

A peer on 0.0.14 refused one notes op every 10 s for hours:
`text edit 0 at char 8874 does not fit a 0-char text`, on
`tasks/sync-pairing-relay/notes.md` in a fresh mirror of this Mac's `txtodo` workspace.
Every later op from this Mac, in every file of that workspace, waited behind it.

Cause, checked in this Mac's `.txtodo/oplog.db`: that file has exactly one op, a 2026-09-22
append at char 8874. No op ever wrote the first 8874 chars. Before notes-sync (v0.0.8),
`NotesActor::open` adopted disk bytes with no op. Since then it seeds only when disk differs
from the projection, and here they matched, so the base never became an op. There is no
snapshot transport, so wiping or rejoining the mirror replays the same op and sticks again.

Scan of this workspace (first op size vs the file's git size before it): 1 file of 257.

## Design

Two parts, both in `txtodo-daemon`, no wire change:

1. **Lenient import.** `NotesActor::import_ops` applies each op in order and skips one that does
   not fit, with a `sync_op_skipped` warn, the way `FileActor::on_sync_ops` has done for todo.txt
   since sync-poison-op. Every op still goes into the log, so heads stay dense and acks move on.
   Only the applied ops reach the Loro mirror.
2. **Repair op at open.** After the disk seed, `open` replays the file's log from empty the same
   lenient way. If that text differs from the projection, the log is missing something, so it
   commits one `NotesEdit` = `diff(replayed, projection)`. The origin's own text does not change:
   the op only makes its log replay to its file again. A fresh peer replays the same ops the same
   way, reaches the same "replayed" text, and the repair lands it on the full file.

`notes_history::replay` uses the same leniency, so checkout and undo work on a repaired file.

Rejected: a new whole-text `OpKind` (set the text to X). More robust to op order, but it is a
wire change: a 0.0.14 peer would fail to decode the whole `Ops` message, not just one op. The
repair op is an ordinary `NotesEdit`, so an old peer decodes it (and stays stuck on the bad op
before it, as today, until it upgrades).

## Known gaps

- The repair fits a peer only if its lenient text before the repair equals the origin's. True
  when one device wrote the file (this bug). With edits from several devices applied in another
  order, the repair can miss; the peer then skips it (logged) and keeps its own text. Same class
  as the existing "notes ops apply in arrival order, no transform" gap in notes-sync.
- A 0.0.14 peer stays stuck until it runs 0.0.15.

## As built

- 2026-09-27: `b25c8e5` moves `NotesActor`'s sync tests to `notes_actor_sync_tests.rs` (line
  budget). `77a79a9` makes import and replay skip ops that don't fit. `8072acb` adds the repair op
  at open. `d5d1d2d` is release 0.0.15.
- `notes_repair.rs` holds the shared lenient apply, `replay_leniently` (it reports whether it
  reached the end of the log, so a cut-off replay never makes a repair), and `repair_edits`.
- `NotesActor::open` repairs only text from this device's own projection. A text restored from a
  peer's mirror snapshot has no op behind it by design, so no repair there. No production path
  ships one today; only `notes_actor_tests.rs`'s `paired()` does.
- Also changed: a notes mirror that won't restore (an op since its snapshot that doesn't fit)
  now rebuilds from the projection. Before, the open failed, and every peer op for that file
  was refused.
- Checked once against a copy of this Mac's real `oplog.db`: `tasks/sync-pairing-relay/notes.md`
  got 1 repair op, and a fresh store importing its 2 ops rebuilt all 9819 bytes.
- Not checked yet: the two real Macs on 0.0.15. Expect `notes_log_repaired` once in this Mac's log
  after its daemon restarts, then the other Mac's `sync FAIL ... stuck on ...notes.md` doctor row
  should clear.
