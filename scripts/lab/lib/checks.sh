# scripts/lab/lib/checks.sh — what a scenario must hold: devices converge, nothing duplicated or
# lost, no tripwire in the daemons' logs, doctor clean, daemons alive. Sourced by lab.sh.

# Events that mean sync went wrong or a repair masked it. Any one fails the scenario.
LAB_TRIPWIRES_FAIL=(sync_op_skipped todo_log_repaired notes_log_repaired reconcile_ops_not_replayable
  todo_log_unrepairable mirror_refused_converging)
# Events that faults can cause legitimately (a partition, a kill). Reported, not failed, until a
# few runs show their normal level (tasks/p2p-lab/notes.md).
LAB_TRIPWIRES_WATCH=(lan_sync_ops_refused lan_sync_batch_partly_committed lan_sync_stuck
  peer_open_failed lan_ops_refused workspace_ops_crypto_refused lan_greet_refused
  lan_link_hello_refused)

# snapshot <device>: "sha256  ./path" for every file in the default workspace, .txtodo/ and
# editor temp files left out, sorted so two devices compare line by line.
snapshot() {
  dx "$1" sh -c "cd $WS && find . -path ./.txtodo -prune -o -type f ! -name '*.labtmp' -print |
    LC_ALL=C sort | xargs -r sha256sum" 2>/dev/null
}

# converged <device...>: every device's snapshot is the same, and not empty.
converged() {
  local first dev s
  first=$(snapshot "$1") || return 1
  [ -n "$first" ] || return 1
  for dev in "${@:2}"; do
    s=$(snapshot "$dev") || return 1
    [ "$s" = "$first" ] || return 1
  done
}

# diff_devices <label> <device...>: unified diffs of every file that differs from the first
# device's copy, into diff-<label>.txt.
diff_devices() {
  local label=$1 base=$2 dev f out
  shift 2
  out="$REPORT_DIR/diff-${label// /-}.txt"
  : >"$out"
  for dev in "$@"; do
    for f in $( (snapshot "$base"; snapshot "$dev") | sort | uniq -u | awk '{ print $2 }' | sort -u); do
      {
        echo "=== $f: $base vs $dev"
        diff -u --label "$base/$f" --label "$dev/$f" \
          <(dx "$base" cat "$WS/$f" 2>/dev/null) <(dx "$dev" cat "$WS/$f" 2>/dev/null) || true
      } >>"$out"
    done
  done
}

# expect_converged <label> <secs> <device...>: waits for convergence; on timeout records a
# failure and the diffs, and returns 0 so the scenario goes on. Once one wait in an attempt has
# timed out the verdict is already "fail", so later waits are capped at 60 s: the scenario still
# runs every step and collects everything, without sitting out each full timeout again.
expect_converged() {
  local label=$1 secs=$2 start=$SECONDS
  shift 2
  if [ -f "$REPORT_DIR/state/diverged" ] && [ "$secs" -gt 60 ]; then
    secs=60
  fi
  if wait_for "$secs" converged "$@"; then
    log "check: converged ($label) in $((SECONDS - start))s"
    return 0
  fi
  touch "$REPORT_DIR/state/diverged"
  diff_devices "$label" "$@"
  if [ "$(split_report "$label" "$@")" = split ]; then
    fail "converge: $label: $* still differ after ${secs}s with the same ops, an application bug (splits-${label// /-}.txt, diff-${label// /-}.txt)"
  else
    fail "converge: $label: $* still differ after ${secs}s, no split with matching ops: a delivery bug or still in flight (diff-${label// /-}.txt)"
  fi
}

# split_report <label> <device...>: each device's doctor rows for a file split with a peer (same
# ops, different bytes; ADR 0035) into splits-<label>.txt; prints "split" when any device has one.
# A split is only booked after a quiet second, so a device still syncing shows none.
split_report() {
  local label=$1 dev rows any=""
  shift
  local out="$REPORT_DIR/splits-${label// /-}.txt"
  : >"$out"
  for dev in "$@"; do
    rows=$(tx "$dev" --json doctor |
      jq -r '.[] | select(.name == "sync" and (.detail | test("both hold the same ops"))) | .detail' \
      2>/dev/null || true)
    if [ -n "$rows" ]; then
      any=1
      printf '%s\n' "$rows" | sed "s/^/$dev: /" >>"$out"
    fi
  done
  if [ -n "$any" ]; then
    echo split
  fi
}

# No non-blank line may appear twice in one list: the sync-drift symptom (every line twice).
check_duplicates() {
  local dev=$1 dups
  dups=$(dx "$dev" sh -c "cd $WS && find . -path ./.txtodo -prune -o -name '*.txt' -type f -print |
    while read -r f; do grep -v '^[[:space:]]*\$' \"\$f\" | sort | uniq -d | sed \"s|^|\$f: |\"; done")
  if [ -n "$dups" ]; then
    printf '%s\n' "$dups" >"$REPORT_DIR/duplicates-$dev.txt"
    fail "duplicates: $dev has $(printf '%s\n' "$dups" | wc -l | tr -d ' ') repeated lines (duplicates-$dev.txt)"
  fi
}

# Every token the workload wrote and nothing later removed must still be on the device.
check_no_loss() {
  local dev=$1 st="$REPORT_DIR/state" missing
  touch "$st/added" "$st/touched" "$st/soft"
  sort -u "$st/added" >"$st/expected.all"
  sort -u "$st/touched" "$st/soft" >"$st/excused"
  comm -23 "$st/expected.all" "$st/excused" >"$st/expected"
  dx "$dev" sh -c "cd $WS && find . -path ./.txtodo -prune -o -type f -print | xargs -r cat" |
    tokens_in | sort -u >"$st/present-$dev"
  missing=$(comm -23 "$st/expected" "$st/present-$dev")
  if [ -n "$missing" ]; then
    printf '%s\n' "$missing" >"$REPORT_DIR/lost-$dev.txt"
    fail "lost: $dev is missing $(printf '%s\n' "$missing" | wc -l | tr -d ' ') of $(wc -l <"$st/expected" | tr -d ' ') tokens (lost-$dev.txt)"
  fi
}

# Counts each tripwire event in a device's collected JSON logs.
check_tripwires() {
  local dev=$1 logs="$REPORT_DIR/devices/$1" ev n
  for ev in "${LAB_TRIPWIRES_FAIL[@]}"; do
    n=$(cat "$logs"/logs/* 2>/dev/null | grep -c "\"$ev\"" || true)
    if [ "$n" -gt 0 ]; then
      fail "tripwire: $dev logged $ev $n times"
    fi
  done
  for ev in "${LAB_TRIPWIRES_WATCH[@]}"; do
    n=$(cat "$logs"/logs/* 2>/dev/null | grep -c "\"$ev\"" || true)
    if [ "$n" -gt 0 ]; then
      note "watch: $dev logged $ev $n times"
    fi
  done
  n=$(grep -c "panicked at" "$logs/stderr.log" 2>/dev/null || true)
  if [ "$n" -gt 0 ]; then
    fail "panic: $dev's daemon panicked $n times (devices/$dev/stderr.log)"
  fi
}

# doctor's FAIL rows, minus any the scenario expects (DOCTOR_ALLOW, an extended regex on
# "name detail").
check_doctor() {
  local dev=$1 rows
  rows=$(jq -r '.[] | select(.status == "FAIL") | "\(.name) \(.detail)"' \
    "$REPORT_DIR/devices/$dev/doctor.json" 2>/dev/null || echo "doctor unreadable")
  if [ -n "${DOCTOR_ALLOW:-}" ]; then
    rows=$(printf '%s\n' "$rows" | grep -Ev "$DOCTOR_ALLOW" || true)
  fi
  if [ -n "$rows" ]; then
    fail "doctor: $dev: $(printf '%s' "$rows" | head -1)"
  fi
}

# final_checks <device...>: run after heal_all.
final_checks() {
  local dev
  expect_converged final "${CONVERGE_TIMEOUT:-180}" "$@"
  collect_all "$@"
  for dev in "$@"; do
    check_duplicates "$dev"
    check_no_loss "$dev"
    check_tripwires "$dev"
    check_doctor "$dev"
  done
}
