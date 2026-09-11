#!/usr/bin/env bash
# txtodo feedback — Claude Code PostToolUse hook. Runs budgets.json.commands.feedback.* on the file just
# written (extensions in commands.feedbackExtensions), plus file-length; boundaries on any Cargo.toml.
# Informs via additionalContext; never blocks. Lockstep twin: guardrails/index.ts tool_result.
set -uo pipefail
IN=$(cat); ROOT=$(node -pe 'JSON.parse(process.argv[1]).cwd' "$IN"); cd "$ROOT"
FILE=$(node -pe 'const i=JSON.parse(process.argv[1]).tool_input||{};require("path").relative(process.argv[2],require("path").resolve(process.argv[2],i.file_path||i.notebook_path||""))' "$IN" "$ROOT")
[ -z "$FILE" ] && exit 0
B=.claude/budgets.json; ext="${FILE##*.}"; findings=""
if node -pe 'JSON.parse(require("fs").readFileSync(process.argv[1])).commands.feedbackExtensions.includes(process.argv[2])?"":process.exit(1)' $B "$ext" >/dev/null 2>&1; then
  for k in $(node -pe 'Object.keys(JSON.parse(require("fs").readFileSync(process.argv[1])).commands.feedback).join(" ")' $B); do
    cmd=$(node -pe 'JSON.parse(require("fs").readFileSync(process.argv[1])).commands.feedback[process.argv[2]].replace("{file}",process.argv[3])' $B "$k" "$FILE")
    out=$(bash -c "$cmd" 2>&1) || findings+="[$k] $(echo "$out" | tail -20)"$'\n'
  done
  out=$(.claude/scripts/check-file-length.sh 2>&1) || findings+="$out"$'\n'
fi
case "$FILE" in *Cargo.toml) out=$(.claude/scripts/check-boundaries.sh 2>&1) || findings+="$out"$'\n';; esac
[ -z "$findings" ] && exit 0
node -e 'console.log(JSON.stringify({hookSpecificOutput:{hookEventName:"PostToolUse",additionalContext:"[feedback] fix as you go, never suppress:\n"+process.argv[1]}}))' "$findings"
