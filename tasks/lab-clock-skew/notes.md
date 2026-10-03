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

## Re-run after ADR 0039 (c5dded44, report 20261002-232638-w67844-5462-clock-skew)
- Delivery is fixed: both devices hold the same ops in every file, same counts, same top numbers.
- Still one lost token on both: a1r4n1, a1's editor append to `tasks/lab/notes.md`. a1's log holds
  only 2 notes ops of its own, so the append never became an op on a1: a local notes save, not sync.
- The `todo.txt` split is unchanged.

## 2026-10-03: found and fixed (two of three)

- **todo.txt split (64f420f7).** Not the skew guard as such: the CLI's `Replace` fallback (a `do`
  that reflowed lines) is reconciled like an editor save. The reconciler placed its ops with a
  scratch stamp newer than everything, then relabelled them with the real stamp, which was older
  than b1's +7m lines (merge refused). A replay placed them by the real stamp: a1's move of
  b1r3n5 is stale there and only adds a ghost. Same bytes, different ghosts; b1's next `do`
  anchored on b1r3n5 split the file. Probe: replaying a1's log with only seq 52..62 applied that
  way gave a1's file exactly. Fix: an exact reconcile commits its ops applied by their real stamp
  (`external.rs::as_logged`); when that differs from the render it warns
  `reconcile_placed_older_than_held` and writes what the log says.
- **Lost notes append, skew under the bound (cf5f74ce).** The op existed (seq 51) but the notes
  actor never applied the HLC receive rule and started its clock at zero, so a1's append was
  stamped older than b1's +2m edit it was typed after; the stamp-ordered rebuild (ADR 0034) slot
  it in front, where it did not fit. Fix: `notes_clock.rs` (receive rule on import, newest
  logged stamp at open).
- **Lab (217f13f2).** `set_clock` returned inside libfaketime's 1 s cache, so on some seeds the
  "+7m" round ran at the old time (seed 424242: the skew-guard check failed on 0.0.20 too).
- Tried and reverted (7249bffe, f5b9fe99): hold back peer ops stamped past the bound. A device
  whose own clock is behind then held back every op from a correct one (seed 777, "b1 10 min
  behind" stalled); a device cannot tell whose clock is wrong.
- Lab on cf5f74ce: all 11 scenarios pass (seed 1072683562); clock-skew passes 4 of 5 seeds since
  the `set_clock` fix.

## Still open: skew past the bound (seed 777)

b1's notes edit carries b1's own +7m stamp (its HLC never goes back); a1 refuses the merge, takes
the edit, and its append typed after it is stamped older, slotted in front, and lost on both. Any
local edit typed on lines a peer stamped more than 5 min ahead can do this (a todo.txt move snaps
back, consistently). Rare in practice (needs clocks 5+ min apart and both editing one file).
The fix is a stamp rule: a local edit takes a stamp past the newest one in its document even when
the clock merge is refused. That amends the skew-guard decision (`tasks/model-hlc-skew-guard`,
option A); a human call, not taken for 0.0.21.
