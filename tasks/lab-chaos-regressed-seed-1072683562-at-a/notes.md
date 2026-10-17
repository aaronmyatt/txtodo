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
