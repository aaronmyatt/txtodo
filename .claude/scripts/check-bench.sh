#!/usr/bin/env bash
# Tier-2 perf budgets: each criterion bench's mean must be ≤ its budgets.json.perf key. Runs each
# bench in bencher output mode and compares. Exit 1 if any is over budget. Runner noise is ±30 %;
# the ms numbers are ceilings. A bench over budget is re-run once and the lower of the two means
# counts: one slow sample on a shared runner is noise (CI 2026-09-19), two in a row is not.
# criterion output format:
# https://bheisler.github.io/criterion.rs/book/user_guide/command_line_options.html
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
# "bench_name:perf_key:crate:bench_target" — the two budgeted benches. parse_file_100k is the core
# parse budget (M1); reconcile_10k_one_edit is the daemon reconcile budget (M3).
BENCHES="parse_file_100k:parse100kMs:txtodo-core:parse reconcile_10k_one_edit:reconcile10kMs:txtodo-daemon:reconcile"

# measure <crate> <target> <name>: runs one bench, prints its mean in ns. Prints nothing (and the
# output tail on stderr) when criterion gave no `bench:` token. Shell functions:
# https://www.gnu.org/software/bash/manual/html_node/Shell-Functions.html
measure() {
  local crate=$1 target=$2 name=$3 out ns
  out=$(cd "$ROOT" && cargo bench -p "$crate" --bench "$target" -- --output-format bencher "$name" 2>&1)
  # Only `$name` runs (the filter arg), so the one `bench: <ns> ns/iter` token is ours. Matched by
  # token, not line shape: on a fresh runner criterion interleaves a stderr "missing baseline" error
  # into the same line (CI 2026-09-11) and the `^test … bench:` anchor missed it.
  ns=$(printf '%s\n' "$out" | awk '{ for (i = 1; i < NF; i++) if ($i == "bench:") { gsub(",", "", $(i+1)); print $(i+1) } }' | tail -1)
  if [ -z "$ns" ]; then
    echo "bench-check: could not find $name in bench output" >&2; printf '%s\n' "$out" | tail -5 >&2
  fi
  printf '%s' "$ns"
}

status=0
for entry in $BENCHES; do
  IFS=: read -r name key crate target <<< "$entry"
  max=$(node -pe 'JSON.parse(require("fs").readFileSync(process.argv[1])).perf[process.argv[2]]' "$ROOT/.claude/budgets.json" "$key")
  ns=$(measure "$crate" "$target" "$name")
  if [ -z "$ns" ]; then status=1; continue; fi
  ms=$(( ns / 1000000 ))
  if [ "$ms" -gt "$max" ]; then
    echo "bench-check: $name = ${ms} ms over budget ${max} ms, re-running once"
    retry=$(measure "$crate" "$target" "$name")
    if [ -n "$retry" ] && [ "$retry" -lt "$ns" ]; then ns=$retry; ms=$(( ns / 1000000 )); fi
  fi
  echo "bench-check: $name = ${ms} ms (budget ${max} ms)"
  [ "$ms" -le "$max" ] || status=1
done
exit $status
