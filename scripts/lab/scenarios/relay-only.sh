# scripts/lab/scenarios/relay-only.sh — sourced by scripts/lab/lib/run.sh.
# Two homes behind hard NATs and no LAN: pairing and sync go through the lab relay only.
TOPOLOGY=world
DEVICES=(a1 b1)
CONVERGE_TIMEOUT=300

scenario_setup() {
  # --random-fully on both routers defeats hole punching. LAN stays on in the daemons, as users
  # run them; the LAN cable is pulled below. (With --no-lan the relay auto-dial never runs: it
  # lives in the LAN resync tick, crates/txtodo-daemon/src/relay_autodial.rs.)
  export LAB_NAT_A=hard LAB_NAT_B=hard
}

scenario_main() {
  # Off the shared LAN for good (not recorded, so heal_all leaves it off).
  unplug a1 lan
  unplug b1 lan
  # The topology itself: a1 reaches the relay, and b1 only through it.
  if ! dx a1 ping -c 1 -W 2 10.231.0.10 >/dev/null 2>&1; then
    fail "topology: a1 cannot reach the relay at 10.231.0.10"
    return 1
  fi
  if dx a1 ping -c 1 -W 1 10.231.2.10 >/dev/null 2>&1; then
    fail "topology: a1 reaches b1's home address directly"
  fi

  pair a1 b1 || return 1
  workload 1 6 a1 b1
  expect_converged "through the relay" 180 a1 b1

  crash b1
  workload 2 5 a1
  restart b1
  workload 3 6 a1 b1
}
