/**
 * txtodo guardrails — the Pi half of the harness. Same enforcement as .claude/hooks/{fence,feedback,gate}.sh,
 * against Pi's own events; both read .claude/budgets.json and must stay in lockstep (drift audit diffs them).
 *   fence    tool_call      every outcome is machine-facing: allow, or block with a reason the agent acts on.
 *                           frozen path → block unless the human created budgets.unfreezeSentinel;
 *                           baseline path → block; non-append write to a ledger → block; slice leased by
 *                           another session, or this session already leases a different slice → block.
 *   feedback tool_result    format/lint/file-length findings appended to the result; never blocks.
 *   gate     agent_settled  dirty tree must pass format, lint, typecheck, test, boundaries, file length, diff size;
 *                           failure → pi.sendUserMessage forces another turn. Three identical failures in a row →
 *                           notify and stop re-triggering (loop guard; the human decides). Clean tree also
 *                           releases this session's slice leases.
 *
 * Slice leases: one crate, one session, across every worktree of this repo — git status alone can't tell
 * two sessions apart. A lease file lives under the repo's shared git-common-dir (same path for the main
 * checkout and every linked worktree): <git-common-dir>/txtodo-leases/<crate>.lock = {sessionId, ts, cwd}.
 * Older than LEASE_TTL_MS → abandoned, stops blocking (same spirit as the gate's 3-strike loop guard).
 * Pi extension API: https://github.com/earendil-works/pi/blob/main/packages/coding-agent/docs/extensions.md
 */
import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";
import { execSync } from "node:child_process";
import { existsSync, mkdirSync, readdirSync, readFileSync, unlinkSync, writeFileSync } from "node:fs";
import { join, relative, resolve } from "node:path";

type Budgets = {
  diffLines: number;
  baselinePaths: string[];
  generatedPaths?: string[];
  unfreezeSentinel?: string;
  slices: { root: string; frozenPaths: string[]; appendOnly: string[] };
  commands: Record<string, string | null> & { feedback: Record<string, string>; feedbackExtensions: string[] };
};
const GATE_KEYS = ["format", "lint", "typecheck", "test", "boundaries", "fileLength"] as const;
const GENERATED_FALLBACK = ["Cargo.lock"]; // used only if budgets.json omits generatedPaths (constitution §6)
const STRIKES_MAX = 3;

const loadBudgets = (root: string): Budgets =>
  JSON.parse(readFileSync(join(root, ".claude/budgets.json"), "utf8")) as Budgets;
const globToRegExp = (g: string): RegExp =>
  new RegExp("^" + g.replace(/[.+^${}()|[\]\\]/g, "\\$&").replace(/\*\*/g, "\0").replace(/\*/g, "[^/]*").replace(/\0/g, ".*") + "$");
const hit = (list: string[] | undefined, rel: string): boolean => (list ?? []).some((g) => globToRegExp(g).test(rel));
const sh = (cmd: string, cwd: string): { ok: boolean; out: string } => {
  try { return { ok: true, out: execSync(cmd, { cwd, encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] }) }; }
  catch (e) { const err = e as { stdout?: string; stderr?: string }; return { ok: false, out: `${err.stdout ?? ""}${err.stderr ?? ""}` }; }
};
const sliceOf = (b: Budgets, rel: string): string | null => rel.match(new RegExp(`^${b.slices.root}/([^/]+)/`))?.[1] ?? null;

// ---- slice leases: one crate, one session, shared across every worktree of this repo ------------
const LEASE_TTL_MS = 4 * 60 * 60 * 1000; // 4h abandoned-session cutoff, matching the gate loop guard
type Lease = { sessionId: string; ts: number; cwd: string };
const leaseDir = (root: string): string => {
  const common = sh("git rev-parse --git-common-dir", root);
  return join(resolve(root, common.ok ? common.out.trim() : ".git"), "txtodo-leases");
};
const leasePath = (dir: string, crate: string): string => join(dir, `${crate}.lock`);
const readLease = (p: string): Lease | null => { try { return JSON.parse(readFileSync(p, "utf8")) as Lease; } catch { return null; } };
const freshLease = (l: Lease | null): boolean => l !== null && Date.now() - l.ts < LEASE_TTL_MS;
const writeLease = (dir: string, crate: string, sessionId: string, root: string): void => {
  mkdirSync(dir, { recursive: true });
  writeFileSync(leasePath(dir, crate), JSON.stringify({ sessionId, ts: Date.now(), cwd: root }));
};
const myOtherLease = (dir: string, crate: string, sessionId: string): string | null => {
  let files: string[]; try { files = readdirSync(dir); } catch { return null; }
  for (const f of files) {
    if (!f.endsWith(".lock")) continue;
    const other = f.slice(0, -5); if (other === crate) continue;
    const l = readLease(join(dir, f)); if (l && l.sessionId === sessionId && freshLease(l)) return other;
  }
  return null;
};
const releaseSessionLeases = (root: string, sessionId: string): void => {
  const dir = leaseDir(root); let files: string[]; try { files = readdirSync(dir); } catch { return; }
  for (const f of files) { if (!f.endsWith(".lock")) continue; const p = join(dir, f); const l = readLease(p); if (l && l.sessionId === sessionId) { try { unlinkSync(p); } catch { /* already gone */ } } }
};

export default function (pi: ExtensionAPI) {
  // ---- fence -------------------------------------------------------------------------------
  pi.on("tool_call", async (event, ctx) => {
    if (event.toolName !== "write" && event.toolName !== "edit") return undefined;
    const root = ctx.cwd; const b = loadBudgets(root);
    const input = event.input as { path: string; content?: string; edits?: { oldText: string; newText: string }[] };
    const rel = relative(root, resolve(root, input.path));
    if (rel.startsWith("..")) return undefined; // outside the repo: not ours to fence
    const sentinel = b.unfreezeSentinel ?? null;
    // The unlock is a file only a human creates; the fence refuses to create it on its own behalf.
    const unfrozen = sentinel !== null && existsSync(join(root, sentinel));
    if (sentinel !== null && rel === sentinel) return { block: true, reason: `${sentinel} is the human-owned unfreeze switch; an agent never creates it. Stop and ask the human to run: touch ${sentinel}` };
    if (hit(b.baselinePaths, rel)) return { block: true, reason: `${rel} is a baseline file: tool-only. Shrink it with the prune command in .claude/stack.md, never by hand.` };
    const target = sliceOf(b, rel);
    if (target) {
      const sessionId = ctx.sessionManager.getSessionId();
      const dir = leaseDir(root);
      const theirs = readLease(leasePath(dir, target));
      if (theirs && theirs.sessionId !== sessionId && freshLease(theirs)) return { block: true, reason: `Slice fence: ${target} is leased by another session (claimed ${new Date(theirs.ts).toISOString()}, worktree ${theirs.cwd}). One slice per session, across every worktree of this repo: wait for it to commit and stop, or ask the human to delete ${leasePath(dir, target)} if abandoned.` };
      const mine = myOtherLease(dir, target, sessionId);
      if (mine) return { block: true, reason: `Slice fence: you already lease ${mine}. One slice per session: finish and commit ${mine} first, then start ${target} as its own task.` };
      writeLease(dir, target, sessionId, root);
    }
    if (hit(b.slices.appendOnly, rel)) {
      const cur = existsSync(join(root, rel)) ? readFileSync(join(root, rel), "utf8") : "";
      const pureAppend = event.toolName === "write" ? (input.content ?? "").startsWith(cur) : (input.edits ?? []).every((e) => e.newText.startsWith(e.oldText));
      if (pureAppend) return undefined;
      return { block: true, reason: `${rel} is append-only for agent writes and this edit rewrites existing lines. Re-issue it as a pure append: keep every existing line byte-identical and add your entry at the end.` };
    }
    if (hit(b.slices.frozenPaths, rel) && !unfrozen) return { block: true, reason: `${rel} is a frozen path (budgets.json.slices.frozenPaths). Frozen paths are the machinery that governs you: change the code so it passes, never the rule that judges it. If editing it is genuinely the task the human gave you, stop and ask them to run: touch ${sentinel}` };
    return undefined;
  });
  // ---- feedback ----------------------------------------------------------------------------
  pi.on("tool_result", async (event, ctx) => {
    if (event.toolName !== "write" && event.toolName !== "edit") return undefined;
    const root = ctx.cwd; const b = loadBudgets(root);
    const rel = relative(root, resolve(root, (event.input as { path: string }).path));
    const ext = rel.split(".").pop() ?? "";
    const findings: string[] = [];
    if (b.commands.feedbackExtensions.includes(ext)) {
      for (const [k, cmd] of Object.entries(b.commands.feedback)) { const r = sh(cmd.replace("{file}", rel), root); if (!r.ok) findings.push(`[${k}] ${r.out.trim()}`); }
      const fl = sh(b.commands.fileLength as string, root); if (!fl.ok) findings.push(fl.out.trim());
    }
    if (rel.endsWith("Cargo.toml")) { const r = sh(b.commands.boundaries as string, root); if (!r.ok) findings.push(r.out.trim()); }
    if (!findings.length) return undefined;
    return { content: [...event.content, { type: "text" as const, text: `\n[feedback] fix as you go, never suppress:\n${findings.join("\n")}` }] };
  });
  // ---- gate --------------------------------------------------------------------------------
  pi.on("agent_settled", async (_event, ctx) => {
    if (!ctx.isIdle()) return;
    const root = ctx.cwd; const b = loadBudgets(root);
    if (!sh("git status --porcelain", root).out.trim()) { releaseSessionLeases(root, ctx.sessionManager.getSessionId()); return; } // clean tree: nothing to gate, and this session's slices are free
    const failures: string[] = [];
    for (const k of GATE_KEYS) { const cmd = b.commands[k]; if (!cmd) continue; const r = sh(cmd, root); if (!r.ok) failures.push(`[${k}] \`${cmd}\`\n${r.out.trim().split("\n").slice(-15).join("\n")}`); }
    const changed = diffLines(root, [...(b.generatedPaths ?? GENERATED_FALLBACK), ...b.baselinePaths]);
    if (changed > b.diffLines) failures.push(`[diff] ${changed} changed lines > budget ${b.diffLines}. Split the change and say so.`);
    const strikesFile = join(root, ".git", "setup-gate-strikes");
    if (!failures.length) { if (existsSync(strikesFile)) writeFileSync(strikesFile, ""); return; }
    const reason = failures.join("\n\n");
    const prev = existsSync(strikesFile) ? readFileSync(strikesFile, "utf8").split("\n") : [];
    const strikes = prev[0] === reason.length.toString() ? Number(prev[1] ?? 0) + 1 : 1;
    writeFileSync(strikesFile, `${reason.length}\n${strikes}`);
    if (strikes > STRIKES_MAX) { if (ctx.hasUI) ctx.ui.notify(`Gate still failing after ${STRIKES_MAX} identical rounds; not re-triggering. Fix or split by hand.`, "error"); return; }
    pi.sendUserMessage(`Gate blocked (round ${strikes}/${STRIKES_MAX}). A blocked stop means fix or split, never bypass:\n\n${reason}`);
  });
}

/** Changed lines vs HEAD (tracked) plus whole untracked files, minus generated/baseline paths. */
function diffLines(root: string, exempt: string[]): number {
  const isExempt = (p: string) => exempt.some((g) => globToRegExp(g).test(p));
  let n = 0;
  for (const l of sh("git diff HEAD --numstat", root).out.split("\n").filter(Boolean)) { const [a, d, p] = l.split("\t"); if (!isExempt(p)) n += (Number(a) || 0) + (Number(d) || 0); }
  for (const p of sh("git ls-files --others --exclude-standard", root).out.split("\n").filter(Boolean)) { if (!isExempt(p)) n += readFileSync(join(root, p), "utf8").split("\n").length; }
  return n;
}
