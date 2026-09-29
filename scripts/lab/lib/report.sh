# scripts/lab/lib/report.sh — collecting what each device knows, writing report.md, and filing a
# failure line into the root todo.txt. Sourced by scripts/lab/lab.sh.

# fail / note: one line each into the attempt's failures / notes file, and the run log.
fail() {
  log "FAIL $*"
  printf '%s\n' "$*" >>"$REPORT_DIR/failures"
}

note() {
  log "note: $*"
  printf '%s\n' "$*" >>"$REPORT_DIR/notes"
}

# collect_device <device>: the daemon's stderr and JSON logs, a tar of its default workspace,
# doctor and device list. `docker cp <ctr>:<path> -` streams a tar and works on a stopped
# container too. https://docs.docker.com/reference/cli/docker/container/cp/
collect_device() {
  local dev=$1 d="$REPORT_DIR/devices/$1" c
  c=$(cname "$dev")
  mkdir -p "$d/logs"
  # Strip the colour codes of the daemon's pretty stderr.
  docker logs "$c" 2>&1 | sed $'s/\x1b\\[[0-9;]*m//g' >"$d/stderr.log" || true
  docker cp "$c:$STATE/logs/." "$d/logs" >/dev/null 2>&1 || true
  # A tar, never loose files: a report must not hold a todo.txt a walker could pick up.
  docker cp "$c:$WS" - >"$d/workspace.tar" 2>/dev/null || true
  if is_running "$dev" && ! is_paused "$dev"; then
    tx "$dev" --json doctor >"$d/doctor.json" || true
    tx "$dev" --json device list >"$d/devices.json" || true
    dx "$dev" txtodod --version >"$d/version.txt" 2>&1 || true
  fi
}

collect_all() {
  local dev
  for dev in "$@"; do
    collect_device "$dev"
  done
  if [ "${TOPOLOGY:-lan}" = world ]; then
    docker logs "$(cname relay)" >"$REPORT_DIR/relay.log" 2>&1 || true
  fi
  touch "$REPORT_DIR/collected"
}

# write_report <scenario> <verdict> <seconds>: report.md in the attempt's directory.
write_report() {
  local scenario=$1 verdict=$2 secs=$3 f="$REPORT_DIR/report.md"
  {
    echo "# lab: $scenario — $verdict"
    echo
    echo "- run: $RUN_ID, seed $SEED, ${secs}s"
    echo "- source: $(git -C "$LAB_ROOT" rev-parse --short HEAD)$(git -C "$LAB_ROOT" diff --quiet HEAD -- crates || echo ' + uncommitted crates/ changes'), image $LAB_IMAGE"
    echo "- re-run exactly this: \`scripts/lab/lab.sh run $scenario $SEED\` (add \`LAB_KEEP=1\` to keep the containers)"
    echo
    echo "## Failed checks"
    echo
    if [ -s "$REPORT_DIR/failures" ]; then
      sed 's/^/- /' "$REPORT_DIR/failures"
    else
      echo "- none"
    fi
    if [ -s "$REPORT_DIR/notes" ]; then
      echo
      echo "## Notes (not failures)"
      echo
      sed 's/^/- /' "$REPORT_DIR/notes"
    fi
    for d in "$REPORT_DIR"/diff-*.txt; do
      [ -s "$d" ] || continue
      echo
      echo "## $(basename "$d")"
      echo
      echo '```diff'
      head -80 "$d"
      echo '```'
    done
    echo
    echo "## Steps"
    echo
    echo '```'
    cat "$REPORT_DIR/steps.log" 2>/dev/null
    echo '```'
    echo
    echo "## Files"
    echo
    echo "- \`steps.log\`, \`ops.log\`: what the scenario and the workload did, in order"
    echo "- \`devices/<device>/\`: \`stderr.log\`, \`logs/\` (JSON), \`workspace.tar\`, \`doctor.json\`"
    echo "- \`state/\`: the workload's added / touched / soft token lists"
  } >"$f"
}

# task_line <scenario> <verdict> <reason> <report path>: the template, filled in.
task_line() {
  local reason
  # One short clause: the first failure, trimmed, with nothing todo.txt would read as syntax.
  # The trailing "(file.txt)" pointer goes (the report has it); `|`, `&` and `\` would also
  # break the sed below.
  # shellcheck disable=SC1003 # '\\' is tr's backslash, not an escaped quote
  reason=$(printf '%s' "$3" | tr -s '\n\t' '  ' | sed -e 's/ *([^)]*)$//' | tr -d '()|&\\' |
    sed 's/^-*//' | cut -c1-70)
  sed -e "s|{{scenario}}|$1|g" -e "s|{{verdict}}|$2|g" -e "s|{{seed}}|$SEED|g" \
    -e "s|{{reason}}|$reason|g" -e "s|{{report}}|$4|g" "$LAB_DEPLOY/failure-task.txt.tmpl" |
    grep -v '^#'
}

# The host's txtodo against the backlog. Through the daemon unless LAB_TASK_NO_DAEMON=1 (tests,
# so a scratch backlog is never registered as a workspace). Never starts a daemon.
backlog_txtodo() {
  if [ "${LAB_TASK_NO_DAEMON:-0}" = 1 ]; then
    TXTODO_NO_AUTOSTART=1 txtodo --no-daemon --dir "$LAB_BACKLOG_DIR" "$@"
  else
    TXTODO_NO_AUTOSTART=1 txtodo --dir "$LAB_BACKLOG_DIR" "$@"
  fi
}

# file_task <scenario> <verdict> <reason> <report.md path>: adds the (A) line to the root
# todo.txt, unless an open line for the same scenario (tag lab:<scenario>) is already there.
file_task() {
  local scenario=$1 line open
  line=$(task_line "$1" "$2" "$3" "$(tilde_path "$4")")
  if [ "${LAB_NO_TASK:-0}" = 1 ]; then
    log "task: not filed (LAB_NO_TASK=1): $line"
    return 0
  fi
  open=$(backlog_txtodo --json list "lab:$scenario" 2>/dev/null |
    jq -r 'select(.completed == false) | .raw' 2>/dev/null | head -1 || true)
  if [ -n "$open" ]; then
    log "task: an open line already covers $scenario, not filing another: $open"
    printf '\nAlready filed: `%s`\n' "$open" >>"$4"
    return 0
  fi
  if backlog_txtodo add "$line" >/dev/null 2>&1; then
    log "task: filed in $LAB_BACKLOG_DIR/todo.txt: $line"
  else
    log "task: could not file into $LAB_BACKLOG_DIR/todo.txt: $line"
    notify "could not file the failure line; see the run log"
  fi
}
