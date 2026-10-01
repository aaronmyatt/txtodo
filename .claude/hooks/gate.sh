#!/usr/bin/env bash
# txtodo gate — Claude Code Stop hook. On a dirty tree it runs budgets.json.commands.fast (task
# fast-gate: rustfmt + file-length on changed .rs files, clippy + the nextest fast profile on the
# changed crates, boundaries/version-sync/specs-mirror when their inputs changed, vitest for
# apps/desktop) minus crates leased by another session — a crate mid-edit elsewhere can't fail your
# Stop. The diff-line budget likewise excludes files under another session's leased crates. All of
# it must exit 0/within budget or {"decision":"block"} so the run cannot end.
# Loop guard: after 3 identical failing rounds (.git/setup-gate-strikes) it stops blocking and says so —
# the human decides. Lockstep twin: guardrails/index.ts agent_settled.
# Clean tree also releases this session's slice leases (see fence.sh) so the next session on that
# crate — in this worktree or another — isn't stuck waiting on one that's already done.
set -uo pipefail
IN=$(cat); ROOT=$(node -pe 'JSON.parse(process.argv[1]).cwd' "$IN"); SID=$(node -pe 'JSON.parse(process.argv[1]).session_id||""' "$IN"); cd "$ROOT"
if [ -z "$(git status --porcelain)" ]; then
  node -e '
    const fs=require("fs"),cp=require("child_process"),path=require("path");
    const root=process.argv[1], sid=process.argv[2]; if(!sid) process.exit(0);
    let common; try{ common=cp.execSync("git rev-parse --git-common-dir",{cwd:root,encoding:"utf8"}).trim(); }catch{ process.exit(0); }
    const dir=path.join(path.resolve(root,common),"txtodo-leases");
    let files; try{ files=fs.readdirSync(dir); }catch{ process.exit(0); }
    for(const f of files){ if(!f.endsWith(".lock")) continue;
      try{ if(JSON.parse(fs.readFileSync(path.join(dir,f),"utf8")).sessionId===sid) fs.unlinkSync(path.join(dir,f)); }catch{} }
  ' "$ROOT" "$SID"
  exit 0
fi
B=.claude/budgets.json; fail=""
LEASES=$(node -e '
  const fs=require("fs"),cp=require("child_process"),path=require("path");
  const root=process.argv[1], sid=process.argv[2];
  let common; try{ common=cp.execSync("git rev-parse --git-common-dir",{cwd:root,encoding:"utf8"}).trim(); }catch{ console.log(""); console.log(""); process.exit(0); }
  const dir=path.join(path.resolve(root,common),"txtodo-leases");
  let files; try{ files=fs.readdirSync(dir); }catch{ console.log(""); console.log(""); process.exit(0); }
  const TTL=4*60*60*1000, mine=[], others=[];
  for(const f of files){ if(!f.endsWith(".lock")) continue;
    try{ const l=JSON.parse(fs.readFileSync(path.join(dir,f),"utf8"));
      if((Date.now()-l.ts)>=TTL) continue;
      (l.sessionId===sid?mine:others).push(f.slice(0,-5)); }catch{} }
  console.log(mine.join(" ")); console.log(others.join(" "));
' "$ROOT" "$SID")
OTHERCRATES=$(echo "$LEASES" | sed -n 2p)
# Timings are dropped from a failure: the strike signature below is the failure text's length, and
# "812 ms" vs "1203 ms" must not make two identical failures look different.
cmd=$(node -pe 'JSON.parse(require("fs").readFileSync(process.argv[1])).commands.fast||""' $B)
if [ -n "$cmd" ]; then
  out=$(bash -c "$cmd --exclude-crates \"$OTHERCRATES\"" 2>&1) || fail+="[fast] \`$cmd\`"$'\n'"$(echo "$out" | grep -v '^  ok ' | sed -E 's/ +[0-9]+ ms.*$//' | tail -45)"$'\n\n'
fi
MAX=$(node -pe 'JSON.parse(require("fs").readFileSync(process.argv[1])).diffLines' $B)
EXEMPT=$(node -pe 'const b=JSON.parse(require("fs").readFileSync(process.argv[1]));[...(b.generatedPaths||["Cargo.lock"]),...b.baselinePaths].map(g=>"^"+g.replace(/[.+^${}()|[\]\\]/g,"\\$&").replace(/\*\*/g,".*").replace(/(?<!\.)\*/g,"[^/]*")+"$").join("|")' $B)
OTHERNUMSTATRE=""; for c in $OTHERCRATES; do OTHERNUMSTATRE="${OTHERNUMSTATRE:+$OTHERNUMSTATRE|}"$'\t'"crates/$c/"; done
OTHERPATHRE=""; [ -n "$OTHERCRATES" ] && OTHERPATHRE="^crates/($(echo "$OTHERCRATES" | tr ' ' '|'))/"
n=$(git diff HEAD --numstat | grep -Ev $'\t('"$EXEMPT"')$' | { [ -n "$OTHERNUMSTATRE" ] && grep -Ev "$OTHERNUMSTATRE" || cat; } | awk '{a+=$1+$2} END{print a+0}')
u=$(git ls-files --others --exclude-standard | grep -Ev "^($EXEMPT)$" | { [ -n "$OTHERPATHRE" ] && grep -Ev "$OTHERPATHRE" || cat; } | xargs -I{} wc -l "{}" 2>/dev/null | awk '{a+=$1} END{print a+0}')
[ $((n+u)) -gt "$MAX" ] && fail+="[diff] $((n+u)) changed lines > budget $MAX. Split the change and say so."$'\n'
# Advisory only, never a failure (tasks/tui-revamp/parity-manifest decide line, 2026-09-25): shown to
# the human as a systemMessage on a pass, added to the reason on a block. Not in `fail`, so it never
# counts toward the 3-strike signature. Ref: https://code.claude.com/docs/en/hooks#common-json-fields
PCMD=$(node -pe 'JSON.parse(require("fs").readFileSync(process.argv[1])).commands.parity||""' $B)
ADVICE=""; [ -n "$PCMD" ] && ADVICE=$(bash -c "$PCMD" 2>&1)
if [ -z "$fail" ]; then
  : > .git/setup-gate-strikes
  [ -n "$ADVICE" ] && node -e 'console.log(JSON.stringify({systemMessage:process.argv[1]}))' "$ADVICE"
  exit 0
fi
sig=${#fail}; prev=$(sed -n 1p .git/setup-gate-strikes 2>/dev/null); cnt=$(sed -n 2p .git/setup-gate-strikes 2>/dev/null)
[ "$prev" = "$sig" ] && cnt=$((cnt+1)) || cnt=1; printf '%s\n%s\n' "$sig" "$cnt" > .git/setup-gate-strikes
if [ "$cnt" -gt 3 ]; then echo "gate: still failing after 3 identical rounds; not blocking again. Fix or split by hand." >&2; exit 0; fi
node -e 'console.log(JSON.stringify({decision:"block",reason:"Gate blocked (round "+process.argv[2]+"/3). A blocked stop means fix or split, never bypass:\n\n"+process.argv[1]+(process.argv[3]?"\n"+process.argv[3]:"")}))' "$fail" "$cnt" "$ADVICE"
