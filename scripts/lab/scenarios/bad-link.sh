# scripts/lab/scenarios/bad-link.sh — sourced by scripts/lab/lib/run.sh.
# Two devices on a LAN whose links delay, jitter, drop, duplicate and reorder packets.
TOPOLOGY=lan
DEVICES=(a1 b1)
CONVERGE_TIMEOUT=300

scenario_main() {
  # Pair on a clean link; the bad link is for sync, not the pairing window.
  pair a1 b1 || return 1
  # netem: https://man7.org/linux/man-pages/man8/tc-netem.8.html (reorder needs a delay)
  badlink a1 delay 120ms 60ms loss 5% duplicate 2% reorder 10% 50% ||
    skip "tc netem is not available in this Docker VM"
  badlink b1 delay 80ms 40ms loss 5% duplicate 2% reorder 10% 50%
  workload 1 8 a1 b1
  workload 2 8 a1 b1
  expect_converged "bad link" 300 a1 b1

  # A burst of heavy loss on one side while both edit.
  badlink b1 delay 200ms 100ms loss 30%
  workload 3 6 a1 b1
}
