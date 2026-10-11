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
