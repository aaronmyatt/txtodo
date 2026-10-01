# scripts/lab/scenarios/partition-edits.sh — sourced by scripts/lab/lib/run.sh.
# Adds, do, archive, deletes and same-text edits across a partition, chosen (not random) to hit
# what ADR 0033 and 0034 fixed: adds under a line the other side deleted or moved, two appends to
# one description, two appends to one notes.md. A failure here points at placement or text order.
TOPOLOGY=lan
DEVICES=(a1 b1)

# add_lines <device> <round> <count>: that many `txtodo add`s, each recorded for the no-loss check.
add_lines() {
  local dev=$1 round=$2 n=$3 i tok
  for ((i = 1; i <= n; i++)); do
    tok="t${SEED}${dev}r${round}n${i}"
    if tx "$dev" add "$tok added on $dev +lab" >/dev/null; then
      printf '%s\n' "$tok" | record added
    fi
  done
  ops_log "$dev add x$n round $round"
}

# on <device> <txtodo args...>: one command, logged with its status; a `del` records what it took.
on() {
  local dev=$1 out rc=0
  shift
  out=$(tx "$dev" "$@") || rc=$?
  if [ "$1" = del ]; then
    printf '%s\n' "$out" | tokens_in | record touched
  fi
  ops_log "$dev $* rc=$rc"
}

# The number of a device's root list's last non-blank line.
last_line() {
  lines_of "$1" | tail -1
}

# notes_append <device> <round>: one line added to tasks/lab/notes.md the way an editor saves.
notes_append() {
  local dev=$1 tok="t${SEED}${1}r${2}n9" out rc=0
  out=$(editor_save "$dev" tasks/lab/notes.md append "- $tok on $dev +lab") || rc=$?
  [ "$rc" -eq 0 ] && printf '%s\n' "$tok" | record added
  printf '%s\n' "$out" | clobbered | tokens_in | record touched
  ops_log "$dev editor-append notes.md $tok rc=$rc"
}

# appends <round>: a1 and b1 append to line 1's description, then to the notes, at the same time.
appends() {
  local tok_a="t${SEED}a1r${1}n8" tok_b="t${SEED}b1r${1}n8"
  on a1 append 1 "and $tok_a" &
  on b1 append 1 "and $tok_b" &
  wait
  # Onto an existing line: a concurrent delete may take them, so they are not loss-checked.
  printf '%s\n%s\n' "$tok_a" "$tok_b" | record soft
  notes_append a1 "$1" &
  notes_append b1 "$1" &
  wait
  log "appends: round $1, one description and notes.md on both at once"
}

scenario_main() {
  local last
  pair a1 b1 || return 1
  mkdir -p "$REPORT_DIR/state"
  add_lines a1 1 6
  expect_converged "six shared lines" 60 a1 b1

  partition b1 lan
  # Apart: a1 deletes the last shared line while b1 adds four under it (ADR 0033: each add is
  # anchored on a line the other side deleted), then each completes a different line (`do` moves
  # it to the bottom), both add and archive, and both append to one description and the notes.
  last=$(last_line a1)
  on a1 del "$last" &
  add_lines b1 2 4 &
  wait
  on a1 "do" 1 &
  on b1 "do" 2 &
  wait
  add_lines a1 3 3 &
  add_lines b1 3 3 &
  wait
  on a1 archive &
  on b1 archive &
  wait
  appends 4
  heal_all a1 b1
  expect_converged "edits made apart" 120 a1 b1

  # Connected: one description and the notes edited on both at once, and a delete racing adds
  # under the line it deletes.
  appends 5
  last=$(last_line a1)
  on b1 del "$last" &
  add_lines a1 6 2 &
  wait
}
