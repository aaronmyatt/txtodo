#!/usr/bin/env bash
# Tier-2 perf budget: `parse_file_100k` mean must be ≤ budgets.json.perf.parse100kMs. Runs the criterion
# bench in bencher output mode and compares. Exit 1 over budget. Runner noise is ±30 %; 150 ms is a ceiling.
# criterion output format: https://bheisler.github.io/criterion.rs/book/user_guide/command_line_options.html
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
MAX_MS=$(node -pe 'JSON.parse(require("fs").readFileSync(process.argv[1])).perf.parse100kMs' "$ROOT/.claude/budgets.json")
out=$(cd "$ROOT" && cargo bench -p txtodo-core --bench parse -- --output-format bencher parse_file_100k 2>&1)
# Only parse_file_100k runs (filter arg), so the one `bench: <ns> ns/iter` token is ours. Matched by
# token, not line shape: on a fresh runner criterion interleaves a stderr "missing baseline" error
# into the same line (CI 2026-09-11) and the `^test … bench:` anchor missed it.
ns=$(printf '%s\n' "$out" | awk '{ for (i = 1; i < NF; i++) if ($i == "bench:") { gsub(",", "", $(i+1)); print $(i+1) } }' | tail -1)
[ -n "$ns" ] || { echo "bench-check: could not find parse_file_100k in bench output"; printf '%s\n' "$out" | tail -5; exit 1; }
ms=$(( ns / 1000000 ))
echo "bench-check: parse_file_100k = ${ms} ms (budget ${MAX_MS} ms)"
[ "$ms" -le "$MAX_MS" ]
