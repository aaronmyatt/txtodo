# scripts/lab/lib/run.sh — one scenario attempt, the re-run that tells "fails" from "flaky", and a
# whole run: lock, Docker, image, every scenario, verdicts, failure lines. Sourced by lab.sh.

# What a scenario file may override (at the top level, or in scenario_setup) before its
# containers start.
scenario_defaults() {
  TOPOLOGY=lan
  DEVICES=(a1 b1)
  PROFILE=
  CONVERGE_TIMEOUT=180
  DOCTOR_ALLOW=
  export LAB_IMAGE_A1=$LAB_IMAGE LAB_IMAGE_B1=$LAB_IMAGE LAB_IMAGE_A2=$LAB_IMAGE
  export LAB_PRELOAD_A1="" LAB_PRELOAD_B1="" LAB_PRELOAD_A2=""
  export LAB_LAN=on LAB_NAT_A=easy LAB_NAT_B=easy
}

# The preload that turns on libfaketime for one device (set LAB_PRELOAD_<DEV> to it).
LAB_FAKETIME=/usr/local/lib/libfaketime.so.1

lab_compose() {
  local args=(-p "$PROJECT" -f "$LAB_DEPLOY/compose.$TOPOLOGY.yml")
  if [ -n "$PROFILE" ]; then
    args+=(--profile "$PROFILE")
  fi
  docker compose "${args[@]}" "$@"
}

# skip <reason>: this scenario cannot run on this machine (no netem, no older release).
skip() {
  printf '%s\n' "$*" >"$REPORT_DIR/skipped"
  log "skip: $*"
  exit 3
}

# EXIT trap of an attempt's subshell: collect if the checks never got that far, then tear down.
finish_attempt() {
  if [ -n "${PROJECT_UP:-}" ] && [ ! -f "$REPORT_DIR/collected" ]; then
    collect_all "${DEVICES[@]}" || true
  fi
  if [ "${LAB_KEEP:-0}" = 1 ]; then
    log "keep: containers left up (LAB_KEEP=1): docker exec -it $(cname a1) bash"
    log "keep: remove them with: docker compose -p $PROJECT down -v"
  else
    docker compose -p "$PROJECT" down -v --remove-orphans --timeout 5 >/dev/null 2>&1 || true
    log "teardown: $PROJECT removed"
  fi
}

# attempt <scenario> <n>: the scenario once, in a subshell with its own EXIT trap. Sets VERDICT
# (pass, fail, broke or skip) and REPORT_DIR.
attempt() {
  local scenario=$1 n=$2 rc=0 start=$SECONDS suffix=""
  if [ "$n" -gt 1 ]; then
    suffix="-rerun"
  fi
  REPORT_DIR="$LAB_HOME/reports/$RUN_ID-$scenario$suffix"
  PROJECT="txlab-$RUN_ID-$scenario-$n"
  mkdir -p "$REPORT_DIR/state"
  : >"$REPORT_DIR/failures"
  STEP_LOG="$REPORT_DIR/steps.log"
  log "== $scenario, attempt $n, seed $SEED: $REPORT_DIR"
  (
    scenario_defaults
    # shellcheck source=/dev/null
    source "$LAB_SCENARIOS/$scenario.sh"
    if declare -F scenario_setup >/dev/null; then
      scenario_setup
    fi
    trap finish_attempt EXIT
    if ! lab_compose up -d >"$REPORT_DIR/compose-up.log" 2>&1; then
      log "compose up failed:"
      tail -20 "$REPORT_DIR/compose-up.log"
      exit 2
    fi
    PROJECT_UP=1
    for dev in "${DEVICES[@]}"; do
      wait_ready "$dev" 90 || exit 1
    done
    scenario_main || log "scenario: stopped early"
    heal_all "${DEVICES[@]}"
    final_checks "${DEVICES[@]}"
  ) || rc=$?
  if [ "$rc" -eq 3 ]; then
    VERDICT=skip
  elif [ -s "$REPORT_DIR/failures" ]; then
    VERDICT=fail
  elif [ "$rc" -ne 0 ]; then
    VERDICT=broke
  else
    VERDICT=pass
  fi
  write_report "$scenario" "$VERDICT" "$((SECONDS - start))"
  log "== $scenario: $VERDICT ($((SECONDS - start))s)"
  STEP_LOG=""
}

# run_scenario <scenario>: an attempt; a failure gets one re-run on the same seed. Records the
# result and files the root todo.txt line for fails, flaky and broke.
run_scenario() {
  local scenario=$1 final first_report reason
  attempt "$scenario" 1
  final=$VERDICT
  first_report="$REPORT_DIR/report.md"
  reason=$(head -1 "$REPORT_DIR/failures")
  if [ "$VERDICT" = broke ]; then
    reason="the lab could not run it, see steps.log"
  fi
  if [ "$VERDICT" = fail ] && [ "${LAB_RERUN:-1}" != 0 ]; then
    attempt "$scenario" 2
    if [ "$VERDICT" = pass ]; then
      final=flaky
    fi
    printf '\n## Re-run on seed %s: %s\n\nSee `%s`.\n' "$SEED" "$VERDICT" "$REPORT_DIR/report.md" >>"$first_report"
  fi
  printf '%s\t%s\t%s\t%s\t%s\t%s\n' "$(date '+%F %T')" "$RUN_ID" "$scenario" "$SEED" "$final" \
    "$first_report" >>"$LAB_HOME/results.tsv"
  RESULTS+=("$scenario=$final")
  case "$final" in
    fail) file_task "$scenario" fails "$reason" "$first_report" ;;
    flaky) file_task "$scenario" flaky "$reason" "$first_report" ;;
    broke) file_task "$scenario" broke "$reason" "$first_report" ;;
  esac
}

lock_holder() {
  local pid
  pid=$(cat "$LAB_HOME/lock/pid" 2>/dev/null || true)
  if [ -n "$pid" ] && kill -0 "$pid" 2>/dev/null; then
    printf '%s\n' "$pid"
    return 0
  fi
  return 1
}

# mkdir is atomic, so of two runs only one gets the lock. A lock whose pid is gone is stale.
acquire_lock() {
  local pid
  mkdir -p "$LAB_HOME"
  if ! mkdir "$LAB_HOME/lock" 2>/dev/null; then
    if pid=$(lock_holder); then
      die "a lab run is already going (pid $pid); see: scripts/lab/lab.sh status"
    fi
    rm -rf "$LAB_HOME/lock"
    mkdir "$LAB_HOME/lock"
  fi
  printf '%s\n' "$$" >"$LAB_HOME/lock/pid"
  printf '%s\n' "$RUN_ID" >"$LAB_HOME/lock/run"
}

release_lock() {
  if [ "$(cat "$LAB_HOME/lock/pid" 2>/dev/null)" = "$$" ]; then
    rm -rf "$LAB_HOME/lock"
  fi
}

# Scenario names from "all" or a comma-separated list; dies on an unknown one.
resolve_scenarios() {
  local s
  if [ -z "$1" ] || [ "$1" = all ]; then
    SCENARIOS=("${LAB_ALL_SCENARIOS[@]}")
    return
  fi
  IFS=, read -r -a SCENARIOS <<<"$1"
  for s in "${SCENARIOS[@]}"; do
    [ -f "$LAB_SCENARIOS/$s.sh" ] || die "no scenario '$s'; scripts/lab/lab.sh list"
  done
}

# lab_run [scenario|all|a,b] [seed]
lab_run() {
  local s
  resolve_scenarios "${1:-all}"
  SEED=${2:-$((RANDOM * 32768 + RANDOM))}
  RUN_ID=${LAB_RUN_ID:-$(date +%Y%m%d-%H%M%S)}
  RUN_DIR="$LAB_HOME/runs/$RUN_ID"
  LAB_BACKLOG_DIR=${LAB_BACKLOG_DIR:-$(lab_main_checkout)}
  RESULTS=()
  mkdir -p "$RUN_DIR"
  acquire_lock
  trap release_lock EXIT
  log "lab: run $RUN_ID, seed $SEED, scenarios: ${SCENARIOS[*]}"
  log "lab: failures are filed into $LAB_BACKLOG_DIR/todo.txt"
  if ! ensure_docker; then
    log "lab: skipped, Docker is not running and did not start"
    notify "run $RUN_ID skipped: Docker did not start"
    return 0
  fi
  down_leftovers
  if ! ensure_current_image; then
    REPORT_DIR=$RUN_DIR
    file_task build broke "the lab image did not build" "$RUN_DIR/build-current.log"
    notify "run $RUN_ID: the lab image did not build"
    return 1
  fi
  for s in "${SCENARIOS[@]}"; do
    run_scenario "$s"
  done
  prune_images || true
  log "lab: run $RUN_ID done: ${RESULTS[*]}"
  notify "run $RUN_ID: ${RESULTS[*]}"
}

# lab_start [scenario|all] [seed]: lab_run in the background; returns at once.
lab_start() {
  local pid run_id
  if pid=$(lock_holder); then
    echo "lab: a run is already going (pid $pid); scripts/lab/lab.sh status"
    return 1
  fi
  run_id=$(date +%Y%m%d-%H%M%S)
  mkdir -p "$LAB_HOME/runs/$run_id"
  # nohup keeps the run alive when the terminal that started it closes.
  # https://man7.org/linux/man-pages/man1/nohup.1.html
  LAB_RUN_ID=$run_id nohup "$LAB_SCRIPTS/lab.sh" run "$@" \
    >"$LAB_HOME/runs/$run_id/run.log" 2>&1 </dev/null &
  echo "lab: run $run_id started in the background (pid $!)"
  echo "  log:     $LAB_HOME/runs/$run_id/run.log"
  echo "  status:  scripts/lab/lab.sh status"
  echo "  results: $LAB_HOME/results.tsv; a failure files an (A) line in the root todo.txt"
}

lab_status() {
  local pid
  if pid=$(lock_holder); then
    echo "running: run $(cat "$LAB_HOME/lock/run") (pid $pid)"
    echo "  log: $LAB_HOME/runs/$(cat "$LAB_HOME/lock/run")/run.log"
    tail -3 "$LAB_HOME/runs/$(cat "$LAB_HOME/lock/run")/run.log" 2>/dev/null | sed 's/^/  | /'
  else
    echo "idle"
  fi
  if [ -s "$LAB_HOME/results.tsv" ]; then
    echo
    echo "last results (when, run, scenario, seed, verdict):"
    tail -n "${LAB_STATUS_LINES:-12}" "$LAB_HOME/results.tsv" | cut -f1-5 | column -t -s $'\t'
  fi
}

lab_list() {
  local f
  for f in "$LAB_SCENARIOS"/*.sh; do
    printf '%-14s %s\n' "$(basename "$f" .sh)" "$(sed -n '2s/^# *//p' "$f")"
  done
}

# Leftover containers, old images, and reports and run logs older than 14 days that no line in
# the backlog still points at.
lab_clean() {
  local d backlog
  backlog="${LAB_BACKLOG_DIR:-$(lab_main_checkout)}/todo.txt"
  if docker info >/dev/null 2>&1; then
    down_leftovers
    prune_images || true
  fi
  for d in "$LAB_HOME"/reports/*/ "$LAB_HOME"/runs/*/; do
    [ -d "$d" ] || continue
    if [ -n "$(find "$d" -maxdepth 0 -mtime +14)" ] && ! grep -qF "$(basename "$d")" "$backlog" 2>/dev/null; then
      rm -rf "$d"
    fi
  done
  echo "lab: cleaned"
}
