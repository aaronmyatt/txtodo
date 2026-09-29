# scripts/lab/scenarios/clock-skew.sh — sourced by scripts/lab/lib/run.sh.
# b1's clock runs 10 min behind, 2 min ahead, then 7 min ahead (past the 5 min limit) and is fixed.
TOPOLOGY=lan
DEVICES=(a1 b1)
CONVERGE_TIMEOUT=240
# libfaketime on b1 only (its daemon and its CLI). MAX_PEER_SKEW_AHEAD_MS is 5 min
# (crates/txtodo-model/src/hlc.rs).
export LAB_PRELOAD_B1=$LAB_FAKETIME
# A peer clock row may still read "ahead" or "behind" from its last sample after the fix.
DOCTOR_ALLOW='^peer .*clock (ahead|behind)'

a1_flags_b1_ahead() {
  tx a1 --json doctor | jq -e '.[] | select(.name == "peer" and (.detail | test("ahead")))' >/dev/null
}

scenario_main() {
  pair a1 b1 || return 1

  set_clock b1 -10m
  workload 1 6 a1 b1
  expect_converged "b1 10 min behind" 120 a1 b1

  set_clock b1 +2m
  workload 2 6 a1 b1
  expect_converged "b1 2 min ahead" 120 a1 b1

  # Past the limit: a1 should refuse b1's sessions and say why in doctor. Both keep editing.
  set_clock b1 +7m
  workload 3 6 a1 b1
  if ! wait_for 60 a1_flags_b1_ahead; then
    fail "skew guard: a1's doctor never showed b1 as ahead with b1 7 min ahead"
  fi

  # NTP fixes b1. Its edits from while it was ahead carry stamps up to 7 min ahead, so a1 may
  # only take them once its own clock passes the limit: allow for that.
  set_clock b1 +0
  workload 4 6 a1 b1
  expect_converged "b1 clock fixed" 420 a1 b1
}
