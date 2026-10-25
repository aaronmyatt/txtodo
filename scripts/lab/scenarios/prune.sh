# scripts/lab/scenarios/prune.sh — sourced by scripts/lab/lib/run.sh.
# Three devices; b1 goes away (lid shut) past the quiet threshold, a1 and a2 drop it and rotate the
# group key (ADR 0044), b1 wakes and learns it was dropped, then pairs with a1 again and every
# device ends with every line, the ones b1 made while dropped included.
TOPOLOGY=lan
DEVICES=(a1 b1 a2)
PROFILE=three
CONVERGE_TIMEOUT=180
# The daemons' prune_quiet_after (deploy/lab/compose.lan.yml): seconds, not the week it is by default.
export LAB_PRUNE_QUIET_AFTER=25s

# dropped_on <device>: its device list has a removed peer (b1, here the only one that can be).
dropped_on() {
  local n
  n=$(tx "$1" --json device list | jq -s '[.[] | select(.removed)] | length') || return 1
  [ "${n:-0}" -ge 1 ]
}

# knows_it_was_dropped <device>: doctor's `group` row is a FAIL (Health.dropped_from_group).
knows_it_was_dropped() {
  tx "$1" --json doctor | jq -e '.[] | select(.name == "group" and .status == "FAIL")' >/dev/null
}

scenario_main() {
  pair a1 b1 || return 1
  pair a1 a2 || return 1
  workload 1 4 a1 b1 a2
  expect_converged "three paired" 120 a1 b1 a2

  # b1 sleeps. Its session closes after 20 s of silence, then 25 s more make it quiet.
  freeze b1
  if ! wait_for 150 dropped_on a1; then
    fail "prune: a1 did not drop the sleeping b1 within 150s"
    return 0
  fi
  if ! wait_for 90 dropped_on a2; then
    fail "prune: a2 did not drop the sleeping b1 within 90s"
    return 0
  fi
  log "prune: a1 and a2 dropped b1"

  # a1 and a2 carry on under the new key; b1 wakes and edits on its own.
  workload 2 6 a1 a2
  expect_converged "a1 and a2 without b1" 90 a1 a2
  thaw b1
  workload 3 3 b1
  if ! wait_for 90 knows_it_was_dropped b1; then
    fail "prune: b1 never learned it was dropped (doctor group row)"
  fi

  # Pairing with one device is enough: a1 relays b1's ops to a2.
  pair a1 b1 || return 1
  expect_converged "b1 back" 150 a1 b1 a2
}
