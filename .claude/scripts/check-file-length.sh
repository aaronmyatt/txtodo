#!/usr/bin/env bash
# Tier-3 check: no .rs file under crates/ may exceed budgets.json.fileLines.
# Exits 1 and lists offenders. Generated files are exempt (none yet; add paths to EXEMPT).
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
MAX=$(node -p 'JSON.parse(require("fs").readFileSync(process.argv[1])).fileLines' "$ROOT/.claude/budgets.json")
EXEMPT='^$'   # regex of exempt paths, e.g. 'crates/txtodo-proto/src/generated/'
status=0
while IFS= read -r f; do
  n=$(wc -l < "$f")
  if [ "$n" -gt "$MAX" ]; then echo "file-length: $f has $n lines (max $MAX)"; status=1; fi
done < <(find "$ROOT/crates" -name '*.rs' -not -path '*/target/*' | grep -Ev "$EXEMPT")
exit $status
