#!/usr/bin/env bash
# scripts/lab/watch.sh — the P2P lab run after the fact on main's commits, in the background
# (tasks/lab-watch/notes.md). The work in the main checkout is never slowed: the commit only
# queues, the source under test sits in a worktree outside the repo, the image builds in Docker.
#
#   scripts/lab/watch.sh start            start the background worker (one per machine)
#   scripts/lab/watch.sh stop             stop it, and the run it has going
#   scripts/lab/watch.sh status           worker, queue, the last commit tested and its verdicts
#   scripts/lab/watch.sh queue [commit]   queue a commit (the post-commit hook does this)
#   scripts/lab/watch.sh once             test the newest queued commit now, in the foreground
#
# Each tested commit gets a full run (one seed, no re-runs). Against the last commit tested: a
# scenario that went from pass to fail is re-run once, bisected over the commits in between, and
# filed in the root todo.txt with the first bad commit; one that went from fail to pass is a
# notification. Verdicts: ~/.local/state/txtodo-lab/watch/verdicts.tsv.
#
# Environment: LAB_WATCH_SEED, LAB_WATCH_SCENARIOS (all, or a,b), LAB_WATCH_POLL (s), and the
# lab's own (TXTODO_LAB_HOME, LAB_NO_TASK, LAB_NO_NOTIFY). Docker Desktop's CPU limit caps it.
set -euo pipefail

# bash 4+ for mapfile, as lab.sh. https://www.gnu.org/software/bash/manual/bash.html#index-BASH_005fVERSINFO
if [ "${BASH_VERSINFO[0]}" -lt 4 ]; then
  for b in /opt/homebrew/bin/bash /usr/local/bin/bash; do
    if [ -x "$b" ]; then
      exec "$b" "$0" "$@"
    fi
  done
  echo "watch: needs bash 4 or newer (brew install bash)" >&2
  exit 1
fi

LAB_SCRIPTS=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
for lib in common images device workload faults checks report run watch; do
  # shellcheck source=/dev/null
  source "$LAB_SCRIPTS/lib/$lib.sh"
done

main() {
  case "${1:-help}" in
    start) watch_start ;;
    stop) watch_stop ;;
    status) watch_status ;;
    queue) watch_queue "${2:-HEAD}" ;;
    once) watch_once || echo "watch: nothing queued" ;;
    loop) watch_loop ;;
    *) sed -n '2,/^set -euo/p' "${BASH_SOURCE[0]}" | sed -e '$d' -e 's/^# \{0,1\}//' ;;
  esac
}

# One line, so bash has read all of it before it runs: editing this file while the worker runs
# cannot make it read a half-written line. https://www.gnu.org/software/bash/manual/bash.html#Shell-Operation
main "$@"; exit $?
