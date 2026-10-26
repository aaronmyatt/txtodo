# scripts/lab/scenarios/partition-lists.sh — sourced by scripts/lab/lib/run.sh.
# Named lists (task two-kinds-of-workspace, ADR 0043): both devices make the list `work` while
# apart, add to it, and meet again. The name gives the list its id, so there is nothing to offer or
# accept: it must end as one list holding every line, once. A list made on one device after they
# are paired reaches the other with no accept. A failure here points at the list id, the own-device
# gate, or the offer that makes a list on the receiver.
TOPOLOGY=lan
DEVICES=(a1 b1)

LISTS=$STATE/lists

# adds <device> <list> <round> <count>: that many `txtodo --list <list> add`s, each recorded for
# the no-loss check.
adds() {
  local dev=$1 list=$2 round=$3 n=$4 i tok
  for ((i = 1; i <= n; i++)); do
    tok="t${SEED}${dev}r${round}n${i}"
    if tx "$dev" --list "$list" add "$tok added on $dev +lab" >/dev/null; then
      printf '%s\n' "$tok" >>"$REPORT_DIR/state/added"
    fi
  done
}

# both <list> <round> <count>: a1 and b1 add to the list at once.
both() {
  mkdir -p "$REPORT_DIR/state"
  adds a1 "$1" "$2" "$3" &
  adds b1 "$1" "$2" "$3" &
  wait
  log "adds: $1 round $2, $3 lines on each device at once"
}

# has_list <device> <name>: the device lists the named list (`txtodo lists` prints name first).
has_list() {
  tx "$1" lists | awk -v n="$2" '$1 == n { found = 1 } END { exit !found }'
}

scenario_main() {
  pair a1 b1 || return 1
  mkdir -p "$REPORT_DIR/state"

  # Apart, each side makes `work` and fills it: nothing has told either about the other's.
  partition b1 lan
  tx a1 lists new work >/dev/null || fail "lists new work failed on a1"
  tx b1 lists new work >/dev/null || fail "lists new work failed on b1"
  both work 1 5
  heal_all a1 b1

  # From here every check looks at the list, not at the default workspace.
  WS=$LISTS/work
  expect_converged "a list both made while apart" 120 a1 b1

  # Together again: more at once, then apart once more.
  both work 2 5
  expect_converged "adds to a list with both attached" 60 a1 b1
  partition b1 lan
  both work 3 5
  heal_all a1 b1
  expect_converged "adds to a list made apart" 120 a1 b1

  # A list made on one device after pairing arrives on the other, unasked.
  tx a1 lists new scratch >/dev/null || fail "lists new scratch failed on a1"
  tx a1 --list scratch add "t${SEED}scratch jotted on a1 +lab" >/dev/null
  if ! wait_for 60 has_list b1 scratch; then
    fail "list scratch never reached b1"
  fi
  # And exactly one `work` on each side, not two lists that happen to share a name.
  for dev in a1 b1; do
    local n
    n=$(tx "$dev" lists | awk '$1 == "work" { c++ } END { print c + 0 }')
    [ "$n" = 1 ] || fail "$dev lists $n lists named work, not one"
  done
}
