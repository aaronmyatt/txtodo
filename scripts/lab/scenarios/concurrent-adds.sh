# scripts/lab/scenarios/concurrent-adds.sh — sourced by scripts/lab/lib/run.sh.
# Both devices add and complete lines at the same moment, across a partition: one order everywhere.
# Only CLI adds and `do`: no deletes, replaces or editor saves, so a failure here is about order
# (task insert-order), not the other open bugs the random workload also hits.
TOPOLOGY=lan
DEVICES=(a1 b1)

# adds <device> <round> <count>: that many `txtodo add`s, each recorded for the no-loss check.
adds() {
  local dev=$1 round=$2 n=$3 i tok
  for ((i = 1; i <= n; i++)); do
    tok="t${SEED}${dev}r${round}n${i}"
    if tx "$dev" add "$tok added on $dev +lab" >/dev/null; then
      printf '%s\n' "$tok" >>"$REPORT_DIR/state/added"
    fi
  done
}

# both <round> <count>: a1 and b1 add at once.
both() {
  mkdir -p "$REPORT_DIR/state"
  adds a1 "$1" "$2" &
  adds b1 "$1" "$2" &
  wait
  log "adds: round $1, $2 lines on each device at once"
}

scenario_main() {
  pair a1 b1 || return 1
  # Into an empty list, then after the same last line, over and over.
  both 1 5
  expect_converged "concurrent adds" 60 a1 b1
  both 2 5
  expect_converged "concurrent adds again" 60 a1 b1

  # Apart, then back: each side's run of adds meets the other's after one shared line.
  partition b1 lan
  both 3 5
  heal_all a1 b1
  expect_converged "adds made apart" 120 a1 b1

  # `do` moves a line after the last one: both complete a different line at once, and add.
  tx a1 "do" 1 >/dev/null &
  tx b1 "do" 2 >/dev/null &
  wait
  both 4 3
}
