# Lab chaos, seed 1072683562

## What failed (2026-10-17, agent triage)

- 20261011-181141 (confirm run at ac5182a5, filed this line) and 20261011-180516: only the doctor
  check failed. a1 and b1 booked a same-ops split on `default/todo.txt` while a2's backlog
  merged; then the files agreed (convergence passed, all three tars byte-identical). The lab
  read doctor 0.3–0.5 s after the last commit. A split clears only on the next digest, sent a
  quiet second (`DIGEST_QUIET`) after a commit, so it was still booked: a timing false alarm.
- 20261011-164520 (0fa02600, same seed, filed as "flaky"): a real one. a1 and b1 hold the same
  ops and stay apart for good, 6 bytes on one done line: b1 has ` pri:C`
  (`x 2026-10-11 2026-10-11 t…a2r5n3 jasper timber +lab @a2 pri:C`). The known gap in
  `tasks/partition-converge/notes.md` (2026-10-02 `do` of a prioritized line): "a concurrent
  priority change on a done line can meet the same mismatch from the other side".
- Since 2026-10-11 13:00: chaos passed 28 times, failed 5.

## Plan

1. Lab: after converging, wait (bounded) for doctor's same-ops splits to clear before collecting,
   so a split booked mid-merge does not fail a run whose files agree. A split still booked after
   the wait still fails.
2. Daemon: replay a1's and b1's logs from the 164520 tars (both hold the same ops), shrink to the
   ops whose order changes the bytes, and fix the order-dependence. A throwaway probe in the
   session scratchpad, as for `chaos-replay-order`; the case it finds becomes a unit test.

## As built (2026-10-17)

- Lab 1b3752f5: `final_checks` runs `settle_splits` (up to `SPLIT_SETTLE`, 10 s) before
  collecting, so a split booked mid-merge that clears on the next digest no longer fails a run.
- Daemon 9d712484: the probe (both logs replayed in their own order, then shrunk) brought 246 ops
  down to 3 on one task: `Priority = C`, `Completed = true` (moves C into `pri:C`), then a newer
  `Priority = None`. Landing after the completion, the clear changed nothing on the done line, so
  `pri:C` stayed. A newer clear on a done line now drops every `pri:` the text history and its
  base hold (`TextHistory::clear_pri`, the "none" of `swap_pri`), in any arrival order. Tests:
  `state_converge_pri_tests.rs` (the clear newer than the completion and between, all orders; a
  newer letter still wins over an older clear).
- Sweep: the 969 device logs saved since 2026-10-11, each replayed against its own `todo.txt`,
  with and without the fix. Same results except two b1 logs, whose replays now match their peers:
  this report and 20261011-150317 lan-converge (the same bug, `pri:A`). The 28 logs that never
  replay to their file are all from 2026-10-11 13:24–14:46, before that day's later fixes.
- The probe is not in git (`probe_chaos_tests.rs`, session scratchpad), as before.
- Known gaps: the lab change was not run by hand (the watcher's chaos runs exercise it); a text
  edit that typed its own `pri:` on a done line is still cut, not merged, when a clear lands.
