#!/usr/bin/env bash
# scripts/lab/lab.sh — the P2P container lab (tasks/p2p-lab/notes.md). Local only, for now.
#
#   scripts/lab/lab.sh start [SCENARIO|all|a,b] [SEED]   run in the background, return at once
#   scripts/lab/lab.sh run   [SCENARIO|all|a,b] [SEED]   run in the foreground
#   scripts/lab/lab.sh status                            is a run going; the last results
#   scripts/lab/lab.sh list                              the scenarios
#   scripts/lab/lab.sh clean                             leftover containers, old images, old reports
#
# A run starts Docker Desktop if it is down, builds the image only when its source changed, runs
# each scenario in its own compose project and always tears it down. A failed scenario is re-run
# once on the same seed, then files an (A) line into the main checkout's todo.txt that points at
# its report under ~/.local/state/txtodo-lab/reports/.
#
# Environment:
#   LAB_KEEP=1            leave the containers up after a foreground run (to docker exec in)
#   LAB_NO_TASK=1         never file a todo.txt line, only log it
#   LAB_RERUN=0           no second attempt on a failure (it is then filed as "fails")
#   LAB_BACKLOG_DIR=DIR   file failures into DIR/todo.txt (default: the main checkout)
#   LAB_TASK_NO_DAEMON=1  file with `txtodo --no-daemon` (for a scratch backlog)
#   LAB_OLD_REF=vX.Y.Z    the release old-new pairs with (default: newest with other crates/)
#   LAB_LOG=FILTER        the daemons' TXTODO_LOG (default: info)
#   TXTODO_LAB_HOME=DIR   reports, run logs, results.tsv (default: ~/.local/state/txtodo-lab)
set -euo pipefail

# bash 4+ for mapfile, associative tests and `${var//}` on arrays. macOS ships 3.2 as /bin/bash;
# Homebrew's is newer. https://www.gnu.org/software/bash/manual/bash.html#index-BASH_005fVERSINFO
if [ "${BASH_VERSINFO[0]}" -lt 4 ]; then
  for b in /opt/homebrew/bin/bash /usr/local/bin/bash; do
    if [ -x "$b" ]; then
      exec "$b" "$0" "$@"
    fi
  done
  echo "lab: needs bash 4 or newer (brew install bash)" >&2
  exit 1
fi

LAB_SCRIPTS=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
for lib in common images device workload faults checks report run; do
  # shellcheck source=/dev/null
  source "$LAB_SCRIPTS/lib/$lib.sh"
done

case "${1:-help}" in
  start)
    shift
    lab_start "$@"
    ;;
  run)
    shift
    lab_run "$@"
    ;;
  status) lab_status ;;
  list) lab_list ;;
  clean) lab_clean ;;
  *)
    sed -n '2,10p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
    ;;
esac
