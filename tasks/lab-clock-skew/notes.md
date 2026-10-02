# lab-clock-skew

## Goal

Lab `clock-skew` passes: a1 and b1 end with the same files and no lost token after b1's clock
jumps -10m, +2m, +7m and back.

## Evidence (report 20261002-203222-w67844-18881-clock-skew, seed 1072683562)

Two separate failures.

### 1. Lost tokens: the origin_seq rank shift (ADR 0039)
- Each side is missing exactly the other's last op: a1 holds 29 of b1's 30 ops, b1 holds 43 of
  a1's 44. The missing ones are b1's editor append to `tasks/lab/todo.txt` (b1r4n4) and a1's
  editor append to `tasks/lab/notes.md` (a1r4n1).
- Cause: each file actor has its own HLC. b1 was +7m in round 3, so its `todo.txt` ops carry +7m
  stamps; its first `tasks/lab/todo.txt` op in round 4 (clock back to +0) sorts before them. a1's
  `todo.txt` actor took b1's earlier +2m stamps, its `notes.md` actor did not. Either way a late op
  ranks in the middle of its device's run, the ranks after it shift, the peer gets an op it holds
  for the number it asks (`lan_sync_ops_already_held`) and never asks for the real one again.
- Fix: ADR 0039's build (`tasks/sync-drift`, `@human` until accepted). Nothing to do here first.

### 2. `todo.txt` split with the same ops
- Both logs hold the same 69 `todo.txt` ops. Replaying each log in its own order (seq), from
  empty, gives b1's file for both devices. a1's live file differs: the block b1r3n2 (done),
  b1r4n2, a1r4n3 sits at the end instead of after b1r3n5.
- So a1's live state was not a pure function of its log. Every write a1 logged
  (`projection_written` hash) matches the replay up to seq 64; the first that does not is the
  sync commit of b1's `do` (seq 65..68: SetField + three Moves, one stamp). From there a1 is off.
- Context: from 12:34:47 a1 refused every b1 stamp (`sync_stamp_not_merged`, 419 s ahead, bound
  300 s). So a1's own edits got stamps older than b1 lines it had already applied. The scratch
  stamp a reconcile uses assumes "a fresh tick is newer than every op held"
  (`state_order.rs::scratch_stamp`); under a refused merge that is false.
- Tried, did not reproduce a1's file: (a) replay with a1's groups restamped past the newest stamp
  seen; (b) reconcile groups (`source = external`) applied with the scratch stamp then settled, as
  the live actor does. Both still split at seq 65..68.
- Not tried yet: drive a real `FileActor` with the same sequence (local groups through
  `on_apply`/reconcile, sync groups through `on_sync_ops`) and diff its internal state (ghosts,
  parents, field stamps) with the replay's after seq 64.
- Probe: `probe_cs_tests.rs` (kept out of git; load two `oplog.db`s, replay, step hashes against
  the log's `projection_written`). Copy is in this session's scratchpad only; rewrite from the
  above if lost.

## Open question

If (2) is the skew guard breaking "local ops are newest", the fix is a stamp rule: e.g. a local
op stamps past the newest stamp in its document even when the wall merge is refused. That changes
what the skew guard means (ADR 0033/0034 rules lean on it): likely an ADR, `@human`.
