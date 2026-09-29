# scripts/lab/scenarios/lan-converge.sh — sourced by scripts/lab/lib/run.sh.
# Two devices on one LAN: pair, edit at once, partition, kill -9 mid-edit, heal.
TOPOLOGY=lan
DEVICES=(a1 b1)

scenario_main() {
  pair a1 b1 || return 1
  workload 1 6 a1 b1
  expect_converged "concurrent edits" 90 a1 b1

  # b1 drops off the LAN; both keep editing; it comes back on a new address (plug without --ip).
  partition b1 lan
  workload 2 6 a1 b1
  heal_all a1 b1
  expect_converged "partition healed" 120 a1 b1

  # SIGKILL b1 while both are mid-edit, then start it again.
  workload 3 12 a1 b1 &
  sleep 1
  crash b1
  wait
  restart b1
  workload 4 6 a1 b1
}
