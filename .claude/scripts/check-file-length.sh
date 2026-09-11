#!/usr/bin/env bash
# Tier-3 check: no .rs file under crates/ may exceed budgets.json.fileLines.
# Generated artifacts are exempt (constitution §6) — the list lives in budgets.json.generatedPaths so
# this script, gate.sh and the Pi twin cannot drift. Exits 1 and lists offenders.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
B="$ROOT/.claude/budgets.json"
MAX=$(node -p 'JSON.parse(require("fs").readFileSync(process.argv[1])).fileLines' "$B")
# glob → regex, anchored on the repo-relative path; no generatedPaths means match nothing.
EXEMPT=$(node -p '
  const b=JSON.parse(require("fs").readFileSync(process.argv[1]));
  const g=(b.generatedPaths||[]);
  g.length ? g.map(x=>"^"+x.replace(/[.+^${}()|[\]\\]/g,"\\$&").replace(/\*\*/g,"\0").replace(/\*/g,"[^/]*").replace(/\0/g,".*")+"$").join("|") : "^$";
' "$B")
status=0
while IFS= read -r f; do
  rel="${f#"$ROOT"/}"
  echo "$rel" | grep -Eq "$EXEMPT" && continue
  n=$(wc -l < "$f")
  if [ "$n" -gt "$MAX" ]; then echo "file-length: $rel has $n lines (max $MAX)"; status=1; fi
done < <(find "$ROOT/crates" -name '*.rs' -not -path '*/target/*')
exit $status
