# scripts/lab/scenarios/sleep.sh — sourced by scripts/lab/lib/run.sh.
# A frozen device (laptop lid shut) misses resends while the other edits, then catches up.
TOPOLOGY=lan
DEVICES=(a1 b1)

scenario_main() {
  pair a1 b1 || return 1
  workload 1 5 a1 b1
  expect_converged "before sleep" 90 a1 b1

  # b1 sleeps through more than two resend intervals (RESEND_AFTER is 10 s) while a1 edits.
  freeze b1
  workload 2 8 a1
  sleep 25
  workload 3 4 a1
  thaw b1
  # b1 edits the moment it wakes, before it has caught up.
  workload 4 6 a1 b1
  expect_converged "b1 woke" 120 a1 b1

  # a1 sleeps in the middle of b1's burst of edits.
  workload 5 10 b1 &
  sleep 1
  freeze a1
  sleep 20
  thaw a1
  wait
}
