# Chaos: same ops, another file (replay order)

## Goal
- Two devices holding the same ops must render the same file. Lab chaos still books a same-ops
  split on `todo.txt` about 2 runs in 20 (2026-10-11: 13:30 and 14:46 reports, both re-runs pass).

## Findings 2026-10-11
- Both failures: b1 has one trailing blank line more than a1. Each device's own log, replayed from
  empty with `apply_parking` (`log_repair::replay_from_empty`), gives exactly its own file; the
  two logs hold the same 278 ops in different orders. So the state is order-dependent.
- Probe (`probe_chaos_tests.rs`, kept in the session scratchpad, not in git): load both
  `oplog.db`s from the report's `devices/*/workspace.tar`, bubble a1's order into b1's one
  adjacent swap at a time, and report the swap that changes the bytes; then drop ops greedily
  while the two orders still differ.
- Cause 1, fixed (e6911a86): one commit group split in a device's log (another device's op
  between its halves). Parking grouped by adjacent stamps, so the half that arrived after its
  group was parked whole applied alone. Parked ops are now indexed by stamp; a later chunk of a
  group parked whole joins it. Test `sync_park_tests::a_group_split_by_another_devices_op_still_waits_whole`.
- Cause 2, open: the same reports still differ after the fix. Minimal pair (92 ops, greedy
  minimisation stalls there): device X's group S1 (a `do`: 20 moves) split in b1's log, its last
  `Move T after U` landing after device Y's newer group S2 (a `do`: moves, then
  `BlankInsert after T` and `BlankRemove after V`). Applying S2's `BlankInsert after T` before
  S1's older `Move T` gives 51 lines; after it, 52. So the late older move's rehome
  (`state_rehome.rs`) and the eraser (`state_erase.rs`) settle one blank differently.
- Why groups split in logs at all: `lan_sync_batch_partly_committed` and the per-peer merged
  batches; with three devices a group's halves can come from different peers' batches.

## Next
- Turn the 92-op case into a unit test in `state_converge_tests.rs` (both orders, same bytes), then
  read `rehome_onto` + `settle` for a blank anchored after a task whose older move lands later.
- Or prevent split groups: never commit part of a commit group (hold a group's tail until the rest
  of its stamp arrives). Harder: the receiver cannot tell a group is complete.

## Cause 2, sharper (2026-10-11, later)
- In the 92-op case both devices' groups are split: device Z's older group S1 (a `do`, 20 moves)
  has its last op, `Move T after U`, logged inside device Y's newer group S2 (a `do`: 14 moves,
  `BlankInsert after T`, `BlankRemove after V`). a1 has it between S2's `BlankInsert` and its
  `BlankRemove`; b1 has it before the `BlankInsert`. b1 keeps a blank right after T that a1 does not.
- The state treats one stamp as one commit applied in order (rehome and RGA never skip equal
  stamps), so an older op landing *inside* a group is the case it does not cover.
- Test data: `src/testdata/chaos_split_group_blank.json` (both orders); the failing test
  `state_split_group_tests::an_older_move_inside_a_newer_groups_blank_ops_renders_the_same` is
  `#[ignore]`d; `the_split_group_case_still_differs` guards that it stays a real case.
- Two ways out: (a) settle a group's blank ops the same whichever older op lands inside it
  (state_rehome / state_erase); (b) never commit part of a commit group, so groups never split
  in a log. (b) needs the sender to say a group is complete, a wire change: ADR-sized.

## As built (2026-10-11)
- Cause 2 fixed: `rehome_onto` moved the blocks that followed T's older placement first-to-last;
  each lands right after T's new placement (equal stamps are not skipped), so two blocks of one
  commit swapped (`T, line, blank` instead of `T, blank, line`) and the commit's `BlankRemove`
  claimed another blank. It now moves them last-first. The 92-op case is a plain test now.
- Both failing reports' full logs (20261011-133013, -144604) replay to one file in either order.
- Daemon suite under `--profile ci`: 645 pass. Known gap: the lab not re-run by hand; the watcher
  runs chaos on each main commit, so its next reports are the check.
