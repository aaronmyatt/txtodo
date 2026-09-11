#!/usr/bin/env bash
# txtodo fence — Claude Code PreToolUse hook (https://code.claude.com/docs/en/hooks). Same rules as
# .pi/extensions/guardrails/index.ts (tool_call); both read .claude/budgets.json — keep in lockstep.
#
# Every outcome is machine-facing: allow, or deny with a reason the agent can act on. There is no "ask".
# The guardrails are unchanged in strength — they simply stop interrupting a human to enforce themselves.
#   baseline path (budgets.json.baselinePaths)              → deny, tool-only, no unlock
#   another slice already dirty (crates/<x>/ vs git status) → deny: one slice per session
#   ledger (slices.appendOnly) and the write is a pure append → allow silently
#   ledger, not a pure append                               → deny: re-issue the edit as an append
#   frozen path (slices.frozenPaths)                        → deny, UNLESS the human has created
#                                                             budgets.json.unfreezeSentinel
#   the sentinel itself                                     → deny, always: only a human creates it,
#                                                             which is what keeps the unlock human-owned
#   Bash whose command names a frozen/baseline/sentinel path with a write operator → deny (heuristic;
#   Bash writes are otherwise invisible to this hook, so prefer Edit/Write for machinery files).
# stdin: {cwd, tool_name, tool_input}; stdout: {"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":…}}
set -euo pipefail
exec node -e '
const fs=require("fs"),cp=require("child_process"),path=require("path");
const inp=JSON.parse(fs.readFileSync(0,"utf8")); const root=inp.cwd; const tool=inp.tool_name; const ti=inp.tool_input||{};
const b=JSON.parse(fs.readFileSync(path.join(root,".claude/budgets.json"),"utf8"));
const out=(d,r)=>{console.log(JSON.stringify({hookSpecificOutput:{hookEventName:"PreToolUse",permissionDecision:d,permissionDecisionReason:r}}));process.exit(0)};
const glob=g=>new RegExp("^"+g.replace(/[.+^${}()|[\]\\]/g,"\\$&").replace(/\*\*/g,"\0").replace(/\*/g,"[^/]*").replace(/\0/g,".*")+"$");
const hit=(list,rel)=>(list||[]).some(g=>glob(g).test(rel));
const sliceOf=p=>{const m=p.match(new RegExp("^"+b.slices.root+"/([^/]+)/"));return m?m[1]:null};
const sentinel=b.unfreezeSentinel||null;
// The unlock is a file only a human creates. Present → frozen paths are writable for this session.
const unfrozen=sentinel&&fs.existsSync(path.join(root,sentinel));
if(tool==="Bash"){ // heuristic only: a write operator plus a frozen/baseline/sentinel path literal in the command text
  const cmd=String(ti.command||"");
  // Ordering rule: a path is only WRITTEN if a write operator appears before it in the same segment.
  // Without this, any stderr redirect (2>/dev/null) makes every read of a frozen path look like a write.
  const OPS=/(>>?|\bsed\s+-i\b|\btee\b|\bmv\b|\brm\b|\bcp\b|\btruncate\b|\btouch\b|\binstall\b|\bln\b)/;
  if(!OPS.test(cmd)) process.exit(0);
  const segs=cmd.split(/\||;|&&|\n/);
  const writesTo=t=>segs.some(sg=>{const m=sg.match(OPS);return !!m&&sg.indexOf(t,m.index)>-1});
  if(sentinel&&writesTo(sentinel)) out("deny",`${sentinel} is the human-owned unfreeze switch; an agent never creates it. Ask the human to run: touch ${sentinel}`);
  const lit=g=>g.replace(/\/?\*\*?.*$/,""); const bl=(b.baselinePaths||[]).map(lit).filter(Boolean).find(writesTo);
  if(bl) out("deny",`Bash write touching baseline path ${bl}: tool-only, never by hand. Shrink it with the prune command in .claude/stack.md.`);
  const fr=(b.slices.frozenPaths||[]).map(lit).filter(Boolean).find(writesTo);
  if(fr&&!unfrozen) out("deny",`Bash command writes a frozen path (${fr}). Frozen paths are machinery: change the code, not the rules. If this write is genuinely the task, stop and ask the human to run: touch ${sentinel}`);
  process.exit(0); }
if(!["Edit","Write","MultiEdit","NotebookEdit"].includes(tool)) process.exit(0);
const abs=ti.file_path||ti.notebook_path; if(!abs) process.exit(0);
const rel=path.relative(root,path.resolve(root,abs)); if(rel.startsWith("..")) process.exit(0);
if(sentinel&&rel===sentinel) out("deny",`${sentinel} is the human-owned unfreeze switch; an agent never creates it. Stop and ask the human to run: touch ${sentinel}`);
if(hit(b.baselinePaths,rel)) out("deny",`${rel} is a baseline file: tool-only. Shrink it with the prune command in .claude/stack.md, never by hand.`);
const target=sliceOf(rel);
if(target){ const st=cp.execSync("git status --porcelain",{cwd:root,encoding:"utf8"});
  const dirty=new Set(st.split("\n").filter(Boolean).map(l=>sliceOf(l.slice(3).trim())).filter(Boolean));
  if(dirty.size&&!dirty.has(target)) out("deny",`Slice fence: ${target} is not the active slice (dirty: ${[...dirty].join(", ")}). One slice per session: finish and commit ${[...dirty].join(", ")} first, then start ${target} as its own task.`); }
if(hit(b.slices.appendOnly,rel)){
  const cur=fs.existsSync(path.join(root,rel))?fs.readFileSync(path.join(root,rel),"utf8"):"";
  const edits=tool==="MultiEdit"?(ti.edits||[]):[{old_string:ti.old_string||"",new_string:ti.new_string||""}];
  const pure=tool==="Write"?String(ti.content||"").startsWith(cur):edits.every(e=>String(e.new_string).startsWith(String(e.old_string)));
  if(pure) process.exit(0);
  out("deny",`${rel} is append-only for agent writes and this edit rewrites existing lines. Re-issue it as a pure append: keep every existing line byte-identical and add your entry at the end.`); }
if(hit(b.slices.frozenPaths,rel)&&!unfrozen) out("deny",`${rel} is a frozen path (budgets.json.slices.frozenPaths). Frozen paths are the machinery that governs you: change the code so it passes, never the rule that judges it. If editing it is genuinely the task the human gave you, stop and ask them to run: touch ${sentinel}`);
'
