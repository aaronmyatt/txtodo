#!/usr/bin/env bash
# txtodo fence — Claude Code PreToolUse hook (https://code.claude.com/docs/en/hooks). Same rules as
# .pi/extensions/guardrails/index.ts (tool_call); both read .claude/budgets.json — keep in lockstep.
#   baseline path (budgets.json.baselinePaths)          → deny, no confirmation path
#   another slice already dirty (crates/<x>/ vs git status) → deny: one slice per session
#   ledger (slices.appendOnly) and the write is a pure append → allow silently
#   ledger, not a pure append / frozen path (slices.frozenPaths) → ask
#   Bash whose command names a frozen/baseline path with a write operator → ask/deny (heuristic; Bash
#   writes are otherwise invisible to this hook, so prefer Edit/Write for machinery files).
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
if(tool==="Bash"){ // heuristic only: a write operator plus a frozen/baseline path literal in the command text
  const cmd=String(ti.command||""); if(!/(>>?|\bsed -i|\btee\b|\bmv\b|\brm\b|\bcp\b|\btruncate\b)/.test(cmd)) process.exit(0);
  const lit=g=>g.replace(/\/?\*\*?.*$/,""); const bl=(b.baselinePaths||[]).map(lit).filter(Boolean).find(p=>cmd.includes(p));
  if(bl) out("deny",`Bash write touching baseline path ${bl}: tool-only, never by hand.`);
  const fr=(b.slices.frozenPaths||[]).map(lit).filter(Boolean).find(p=>cmd.includes(p));
  if(fr) out("ask",`Bash command appears to write a frozen path (${fr}). Confirm.`); process.exit(0); }
if(!["Edit","Write","MultiEdit","NotebookEdit"].includes(tool)) process.exit(0);
const abs=ti.file_path||ti.notebook_path; if(!abs) process.exit(0);
const rel=path.relative(root,path.resolve(root,abs)); if(rel.startsWith("..")) process.exit(0);
if(hit(b.baselinePaths,rel)) out("deny",`${rel} is a baseline file: tool-only. Shrink it with the prune command in .claude/stack.md, never by hand.`);
const target=sliceOf(rel);
if(target){ const st=cp.execSync("git status --porcelain",{cwd:root,encoding:"utf8"});
  const dirty=new Set(st.split("\n").filter(Boolean).map(l=>sliceOf(l.slice(3).trim())).filter(Boolean));
  if(dirty.size&&!dirty.has(target)) out("deny",`Slice fence: ${target} is not the active slice (dirty: ${[...dirty].join(", ")}). One slice per session: commit or stash first, or propose a separate task.`); }
if(hit(b.slices.appendOnly,rel)){
  const cur=fs.existsSync(path.join(root,rel))?fs.readFileSync(path.join(root,rel),"utf8"):"";
  const edits=tool==="MultiEdit"?(ti.edits||[]):[{old_string:ti.old_string||"",new_string:ti.new_string||""}];
  const pure=tool==="Write"?String(ti.content||"").startsWith(cur):edits.every(e=>String(e.new_string).startsWith(String(e.old_string)));
  if(pure) process.exit(0);
  out("ask",`${rel} is append-only for agent writes. This edit is not a pure append. Confirm.`); }
if(hit(b.slices.frozenPaths,rel)) out("ask",`${rel} is a frozen path (budgets.json.slices.frozenPaths). Confirm this write.`);
'
