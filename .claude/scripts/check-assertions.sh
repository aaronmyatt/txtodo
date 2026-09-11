#!/usr/bin/env bash
# Tier-3 heuristic: count assert-like tokens per fn body; report fns below budgets.json.assertionsMin.
# Reports only (exit 0) — review decides. False negatives accepted (guard clauses are not counted).
# Body boundaries are found by brace counting, so a `{` inside a string can skew one function.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
MIN=$(node -p 'JSON.parse(require("fs").readFileSync(process.argv[1])).assertionsMin' "$ROOT/.claude/budgets.json")
find "$ROOT/crates" -name '*.rs' -not -path '*/target/*' -print0 | xargs -0 awk -v min="$MIN" '
  FNR==1 { depth=0; infn=0 }
  /^[[:space:]]*(pub(\([a-z]+\))? )?(const |async |unsafe )*fn [A-Za-z_]/ && !infn {
    infn=1; name=$0; sub(/^[^f]*fn /,"",name); sub(/[(<].*/,"",name); start=FNR; count=0; depth=0
  }
  infn {
    if ($0 ~ /(debug_)?assert(_eq|_ne)?!|ensure!|unreachable!|\.expect\(/) count++
    o=gsub(/{/,"{"); c=gsub(/}/,"}"); depth+=o-c
    if (o+c>0 && depth<=0) {
      if (count<min && start!=FNR) printf "assertions: %s:%d fn %s has %d (min %d)\n", FILENAME, start, name, count, min
      infn=0
    }
  }'
exit 0
