# scripts/lab/lib/watch.sh — the background lab worker (tasks/lab-watch/notes.md). Sourced by
# scripts/lab/watch.sh; not run on its own.
#
# A post-commit hook queues main's commits. One worker takes the newest and drops the rest,
# checks it out in one reused detached worktree outside the repo, and runs this checkout's
# harness against that source (LAB_SOURCE). Verdicts are kept by commit. Against the last commit
# it tested: a scenario that went from pass to fail is re-run once on the same commit, then
# bisected over the commits in between and filed with the first bad one; one that went from
# fail to pass is a notification.

LAB_WATCH_DIR="$LAB_HOME/watch"
# The repo whose commits are tested (tests point it at a scratch repo).
LAB_WATCH_REPO=${LAB_WATCH_REPO:-$(lab_main_checkout)}
# Outside every workspace: the host daemon walks the main checkout, and a worktree under it gets
# its lists registered and id:-stamped.
LAB_WATCH_WORKTREE=${LAB_WATCH_WORKTREE:-$LAB_WATCH_DIR/wt}
# One seed for every run, so a verdict that changes is the code, not the workload.
LAB_WATCH_SEED=${LAB_WATCH_SEED:-1072683562}
LAB_WATCH_SCENARIOS=${LAB_WATCH_SCENARIOS:-all}
LAB_WATCH_POLL=${LAB_WATCH_POLL:-30}
# What runs one commit: `<runner> <scenarios> <seed>` with LAB_SOURCE and LAB_RUN_ID set,
# appending rows to $LAB_HOME/results.tsv. Tests swap in a fake one.
LAB_WATCH_RUNNER=${LAB_WATCH_RUNNER:-$LAB_SCRIPTS/lab.sh run}
# when, commit, kind (full|confirm|bisect), run, scenario, verdict, report
LAB_WATCH_VERDICTS="$LAB_WATCH_DIR/verdicts.tsv"

# To stderr: the worker's stdout carries verdicts between its functions; both end in watch.log.
wlog() {
  log "watch: $*" >&2
}

# watch_queue <commit>: appends it to the queue when it touches what the lab builds or runs.
# Does nothing else, so the post-commit hook costs nothing.
watch_queue() {
  local sha
  sha=$(git -C "$LAB_WATCH_REPO" rev-parse --verify --quiet "${1:-HEAD}^{commit}") || return 0
  if ! git -C "$LAB_WATCH_REPO" diff-tree --no-commit-id --name-only -r --root "$sha" -- \
    "${LAB_BUILD_INPUTS[@]}" scripts/lab deploy/lab | grep -q .; then
    return 0
  fi
  mkdir -p "$LAB_WATCH_DIR"
  printf '%s\n' "$sha" >>"$LAB_WATCH_DIR/queue"
}

# The newest queued commit, emptying the queue (newest wins). The mv is atomic: a commit the
# hook appends meanwhile lands in a fresh queue file, never lost.
watch_take_newest() {
  local taking="$LAB_WATCH_DIR/queue.taking.$$" sha
  mv "$LAB_WATCH_DIR/queue" "$taking" 2>/dev/null || return 0
  sha=$(grep -v '^$' "$taking" | tail -1 || true)
  rm -f "$taking"
  printf '%s' "$sha"
}

# The last commit a full run tested, or nothing.
watch_last_tested() {
  [ -s "$LAB_WATCH_VERDICTS" ] || return 0
  awk -F'\t' '$3 == "full" { sha = $2 } END { printf "%s", sha }' "$LAB_WATCH_VERDICTS"
}

# watch_verdict <commit> <scenario>: its verdict from the last full run of that commit.
watch_verdict() {
  [ -s "$LAB_WATCH_VERDICTS" ] || return 0
  awk -F'\t' -v c="$1" -v s="$2" '$2 == c && $3 == "full" && $5 == s { v = $6 } END { printf "%s", v }' \
    "$LAB_WATCH_VERDICTS"
}

# Moves the worktree to <commit>, creating it the first time.
watch_checkout() {
  if [ ! -e "$LAB_WATCH_WORKTREE/.git" ]; then
    mkdir -p "$(dirname "$LAB_WATCH_WORKTREE")"
    git -C "$LAB_WATCH_REPO" worktree prune
    git -C "$LAB_WATCH_REPO" worktree add --quiet --detach "$LAB_WATCH_WORKTREE" "$1"
  else
    git -C "$LAB_WATCH_WORKTREE" checkout --quiet --detach --force "$1"
    git -C "$LAB_WATCH_WORKTREE" clean -fdq
  fi
}

# watch_run <commit> <kind> <scenarios>: one lab run on that commit, waiting for a run someone
# started by hand; records its verdicts and prints them as "scenario verdict" lines.
watch_run() {
  local sha=$1 kind=$2 scenarios=$3 run_id
  while lock_holder >/dev/null; do
    sleep "$LAB_WATCH_POLL"
  done
  watch_checkout "$sha"
  run_id="$(date +%Y%m%d-%H%M%S)-w$$-$RANDOM"
  mkdir -p "$LAB_HOME/runs/$run_id"
  wlog "$kind run $run_id: ${sha:0:8} $scenarios"
  # nice: the lab yields the CPU to the work in the main checkout.
  # https://man7.org/linux/man-pages/man1/nice.1.html
  # shellcheck disable=SC2086 # the runner is a command line on purpose
  LAB_SOURCE=$LAB_WATCH_WORKTREE LAB_RUN_ID=$run_id LAB_NO_TASK=1 LAB_RERUN=0 \
    nice -n 10 $LAB_WATCH_RUNNER "$scenarios" "$LAB_WATCH_SEED" \
    >>"$LAB_HOME/runs/$run_id/run.log" 2>&1 </dev/null || true
  awk -F'\t' -v r="$run_id" -v c="$sha" -v k="$kind" 'BEGIN { OFS = "\t" }
    $2 == r { print $1, c, k, r, $3, $5, $6 }' "$LAB_HOME/results.tsv" 2>/dev/null |
    tee -a "$LAB_WATCH_VERDICTS" | awk -F'\t' '{ print $5, $6 }'
}

# watch_bisect <scenario> <good> <bad>: the first commit after <good> on which <scenario> fails,
# testing only that scenario. Only commits that touch the lab's inputs can change a verdict.
watch_bisect() {
  local scenario=$1 lo=-1 hi mid verdict
  local -a commits
  mapfile -t commits < <(git -C "$LAB_WATCH_REPO" rev-list --reverse "$2..$3" -- \
    "${LAB_BUILD_INPUTS[@]}" scripts/lab deploy/lab)
  hi=$((${#commits[@]} - 1))
  if [ "$hi" -lt 0 ]; then
    printf '%s' "$3"
    return 0
  fi
  while [ $((hi - lo)) -gt 1 ]; do
    mid=$(((lo + hi) / 2))
    verdict=$(watch_run "${commits[$mid]}" bisect "$scenario" | awk '{ print $2 }')
    if [ "$verdict" = pass ]; then lo=$mid; else hi=$mid; fi
  done
  printf '%s' "${commits[$hi]}"
}

# A scenario that went from pass to fail: re-run once, then bisect and file.
watch_regressed() {
  local scenario=$1 good=$2 bad=$3 again first subject report
  again=$(watch_run "$bad" confirm "$scenario" | awk '{ print $2 }')
  if [ "$again" = pass ]; then
    wlog "$scenario failed once at ${bad:0:8}, then passed: flaky, not filed"
    return 0
  fi
  first=$(watch_bisect "$scenario" "$good" "$bad")
  subject=$(git -C "$LAB_WATCH_REPO" log -1 --format=%s "$first")
  report=$(awk -F'\t' -v c="$first" -v s="$scenario" '$2 == c && $5 == s { r = $7 } END { print r }' \
    "$LAB_WATCH_VERDICTS")
  wlog "$scenario regressed at ${first:0:8} ($subject)"
  notify "$scenario regressed at ${first:0:8}: $subject"
  SEED=$LAB_WATCH_SEED
  LAB_BACKLOG_DIR=${LAB_BACKLOG_DIR:-$LAB_WATCH_REPO}
  file_task "$scenario" regressed "at ${first:0:8} $subject" "${report:-$LAB_WATCH_VERDICTS}"
}

# One commit: a full run, then the comparison with the last commit tested.
watch_commit() {
  local sha=$1 prev row scenario verdict was
  local -a rows
  prev=$(watch_last_tested)
  mapfile -t rows < <(watch_run "$sha" full "$LAB_WATCH_SCENARIOS")
  for row in "${rows[@]}"; do
    read -r scenario verdict <<<"$row"
    [ -n "$prev" ] || continue
    was=$(watch_verdict "$prev" "$scenario")
    if [ "$was" = pass ] && [ "$verdict" != pass ]; then
      watch_regressed "$scenario" "$prev" "$sha"
    elif [ -n "$was" ] && [ "$was" != pass ] && [ "$verdict" = pass ]; then
      wlog "$scenario passes from ${sha:0:8} on"
      notify "$scenario passes from ${sha:0:8} on"
    fi
  done
  if [ "${#rows[@]}" = 0 ]; then
    wlog "no verdicts for ${sha:0:8}: did its image build? See its run log"
    notify "no verdicts for ${sha:0:8}; see the watch log"
  elif [ -z "$prev" ]; then
    wlog "${sha:0:8} is the baseline"
  fi
}

# Takes the newest queued commit, if any, and tests it. Returns whether there was one.
watch_once() {
  local sha
  sha=$(watch_take_newest)
  [ -n "$sha" ] || return 1
  if [ "$sha" = "$(watch_last_tested)" ]; then
    wlog "${sha:0:8} already tested"
    return 0
  fi
  watch_commit "$sha"
}

# The worker: forever, bounded by watch-stop.
watch_loop() {
  wlog "worker $$ up: seed $LAB_WATCH_SEED, scenarios $LAB_WATCH_SCENARIOS, worktree $LAB_WATCH_WORKTREE"
  while true; do
    watch_once || sleep "$LAB_WATCH_POLL"
  done
}

watch_pid() {
  local pid
  pid=$(cat "$LAB_WATCH_DIR/pid" 2>/dev/null || true)
  [ -n "$pid" ] && kill -0 "$pid" 2>/dev/null && printf '%s' "$pid"
}

watch_start() {
  local pid
  if pid=$(watch_pid); then
    echo "watch: already running (pid $pid)"
    return 0
  fi
  mkdir -p "$LAB_WATCH_DIR"
  # nohup keeps the worker alive when the terminal that started it closes.
  # https://man7.org/linux/man-pages/man1/nohup.1.html
  nohup "$LAB_SCRIPTS/watch.sh" loop >>"$LAB_WATCH_DIR/watch.log" 2>&1 </dev/null &
  printf '%s\n' "$!" >"$LAB_WATCH_DIR/pid"
  echo "watch: worker started (pid $!); queue a commit with scripts/lab/watch.sh queue [commit]"
  echo "  log: $LAB_WATCH_DIR/watch.log"
}

# Every process under <pid>, deepest last; collected before any is killed, so none is orphaned.
# https://man7.org/linux/man-pages/man1/pgrep.1.html
descendants() {
  local child
  for child in $(pgrep -P "$1" || true); do
    printf '%s\n' "$child"
    descendants "$child"
  done
}

# Stops the worker and the run it has going (a plain kill of the run's pid orphaned its scenario
# shell), then removes that run's containers.
watch_stop() {
  local pid
  if ! pid=$(watch_pid); then
    echo "watch: not running"
    return 0
  fi
  # shellcheck disable=SC2046 # one pid per word
  kill "$pid" $(descendants "$pid") 2>/dev/null || true
  rm -f "$LAB_WATCH_DIR/pid"
  down_leftovers || true
  echo "watch: stopped (pid $pid)"
}

watch_status() {
  local pid last queued
  if pid=$(watch_pid); then echo "watch: running (pid $pid)"; else echo "watch: not running"; fi
  queued=$(grep -c . "$LAB_WATCH_DIR/queue" 2>/dev/null || true)
  echo "  queued: ${queued:-0} (only the newest is tested)"
  last=$(watch_last_tested)
  if [ -n "$last" ]; then
    echo "  last tested: ${last:0:8} $(git -C "$LAB_WATCH_REPO" log -1 --format=%s "$last" 2>/dev/null)"
    awk -F'\t' -v c="$last" '$2 == c && $3 == "full" { printf "    %-16s %s\n", $5, $6 }' \
      "$LAB_WATCH_VERDICTS"
  fi
  [ -s "$LAB_WATCH_DIR/watch.log" ] && tail -3 "$LAB_WATCH_DIR/watch.log" | sed 's/^/  | /'
  return 0
}
