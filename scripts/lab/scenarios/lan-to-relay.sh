# scripts/lab/scenarios/lan-to-relay.sh — sourced by scripts/lab/lib/run.sh.
# Paired on the LAN; the LAN goes away and sync carries on through the relay; the LAN returns.
TOPOLOGY=world
DEVICES=(a1 b1)
CONVERGE_TIMEOUT=300

scenario_main() {
  pair a1 b1 || return 1
  workload 1 6 a1 b1
  expect_converged "on the LAN" 90 a1 b1

  # Both leave the LAN; each can still reach the relay through its own router.
  partition a1 lan 10.231.10.11
  partition b1 lan 10.231.10.12
  workload 2 6 a1 b1
  expect_converged "LAN gone, relay only" 240 a1 b1

  heal_all a1 b1
  workload 3 6 a1 b1
}
