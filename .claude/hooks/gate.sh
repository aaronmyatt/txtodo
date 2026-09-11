#!/usr/bin/env bash
# txtodo gate — Claude Code Stop hook. On a dirty tree, every whole-tree command in budgets.json.commands
# (format, lint, typecheck, test, boundaries, fileLength) must exit 0 and the change must be ≤ diffLines
# (Cargo.lock and baselinePaths exempt). Failure → {"decision":"block"} so the run cannot end.
# Loop guard: after 3 identical failing rounds (.git/setup-gate-strikes) it stops blocking and says so —
# the human decides. Lockstep twin: guardrails/index.ts agent_settled.
set -uo pipefail
IN=$(cat); ROOT=$(node -pe 'JSON.parse(process.argv[1]).cwd' "$IN"); cd "$ROOT"
[ -z "$(git status --porcelain)" ] && exit 0
B=.claude/budgets.json; fail=""
for k in format lint typecheck test boundaries fileLength; do
  cmd=$(node -pe 'JSON.parse(require("fs").readFileSync(process.argv[1])).commands[process.argv[2]]||""' $B "$k"); [ -z "$cmd" ] && continue
  out=$(bash -c "$cmd" 2>&1) || fail+="[$k] \`$cmd\`"$'\n'"$(echo "$out" | tail -15)"$'\n\n'
done
MAX=$(node -pe 'JSON.parse(require("fs").readFileSync(process.argv[1])).diffLines' $B)
EXEMPT=$(node -pe 'const b=JSON.parse(require("fs").readFileSync(process.argv[1]));[...(b.generatedPaths||["Cargo.lock"]),...b.baselinePaths].map(g=>"^"+g.replace(/[.+^${}()|[\]\\]/g,"\\$&").replace(/\*\*/g,".*").replace(/(?<!\.)\*/g,"[^/]*")+"$").join("|")' $B)
n=$(git diff HEAD --numstat | grep -Ev $'\t('"$EXEMPT"')$' | awk '{a+=$1+$2} END{print a+0}')
u=$(git ls-files --others --exclude-standard | grep -Ev "^($EXEMPT)$" | xargs -I{} wc -l "{}" 2>/dev/null | awk '{a+=$1} END{print a+0}')
[ $((n+u)) -gt "$MAX" ] && fail+="[diff] $((n+u)) changed lines > budget $MAX. Split the change and say so."$'\n'
[ -z "$fail" ] && { : > .git/setup-gate-strikes; exit 0; }
sig=${#fail}; prev=$(sed -n 1p .git/setup-gate-strikes 2>/dev/null); cnt=$(sed -n 2p .git/setup-gate-strikes 2>/dev/null)
[ "$prev" = "$sig" ] && cnt=$((cnt+1)) || cnt=1; printf '%s\n%s\n' "$sig" "$cnt" > .git/setup-gate-strikes
if [ "$cnt" -gt 3 ]; then echo "gate: still failing after 3 identical rounds; not blocking again. Fix or split by hand." >&2; exit 0; fi
node -e 'console.log(JSON.stringify({decision:"block",reason:"Gate blocked (round "+process.argv[2]+"/3). A blocked stop means fix or split, never bypass:\n\n"+process.argv[1]}))' "$fail" "$cnt"
