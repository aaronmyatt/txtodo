# insert-order

## Goal

Two devices that add, move or blank-insert lines at the same time end with the same line order.

## Evidence (2026-09-30, the p2p lab)

a1 and b1 each add one line into an empty list at the same moment. Both end with both lines, a1
with b1's on top and b1 with a1's on top, for good. Every lab scenario hits it; it is the first
thing that keeps two files apart.

Cause: `DocState::insert` (`crates/txtodo-daemon/src/state.rs`) puts a line right after its
anchor. A peer's op arrives after the local one, so it lands between the anchor and the local
line on each side. `Move` and `BlankInsert` place the same way. The Loro mirror places the same
way too (`txtodo-crdt/src/to_loro.rs` `insert_index`) and is never consulted for bytes.

## Design

RGA's rule (Roh et al. 2011, "Replicated abstract data types"), on the Vec the actor already has:

- Every entry keeps the HLC of the op that placed it (`Insert`, same-file `Move`,
  `BlankInsert`). Lines read from disk get zero.
- An op placing after anchor A goes right after A, then past every following entry whose HLC is
  greater than its own. A later op was made by a device that had seen this one, or concurrently;
  either way it sorts first. Every device skips the same entries, so the order is a function of
  the ops, not of their arrival order.
- Entries with the op's own HLC are never skipped: they came from the same commit (one tick per
  batch, one device), which every device applies in the same order.
- A local op is always the newest, so it still lands right after its anchor: the CLI, the
  reconciler and every existing test see the same placement as before. That needs the actor's
  clock to be ahead of every stamp it holds, so committing a peer's batch now merges the batch's
  newest stamp (`Hlc::merge`, the receive rule; a peer too far ahead is not merged, as today).
- Concurrent moves of one task: the newest wins (an older same-file `Move` than the task's stamp
  is ignored). Cross-file moves are unchanged.
- The reconciler checks its ops on scratch copies before they are stamped. Scratch ops place as
  the newest possible stamp; the commit re-stamps what they placed with its real HLC.
- At open, `repair_log` already replays the whole log from empty. The replay's stamps are copied
  onto the state when its bytes match; otherwise tasks take theirs by id.

No wire change, no store change: the stamps live in memory and are rebuilt from the log.

Rejected: ordering from the Loro mirror (the design doc's `MovableList`). The mirror is fed each
op as a *local* Loro edit on each device, so its lists diverge the same way; making it the
source would mean syncing Loro updates instead of ops, a protocol change.

Rejected: tie-breaking on TaskId (ULID). A device whose clock is behind mints ids smaller than
the lines it inserts after, so its own adds would land below lines it meant to precede.

## Known gaps

- A replay cut off at `MAX_REPLAY_PAGES` leaves zero stamps after a restart.
- When the log does not rebuild the file and the repair runs, blank lines keep zero stamps.
- An op anchored on a task another device deleted is still skipped (`sync_op_skipped`): deletes
  remove the entry, there is no tombstone to anchor on.
