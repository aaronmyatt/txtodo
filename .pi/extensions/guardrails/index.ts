/**
 * txtodo guardrails — the Pi half of the harness. Same enforcement as .claude/hooks/{fence,feedback,gate}.sh,
 * against Pi's own events; both read .claude/budgets.json and must stay in lockstep (drift audit diffs them).
 *   fence    tool_call      every outcome is machine-facing: allow, or block with a reason the agent acts on.
 *                           frozen path → block unless the human created budgets.unfreezeSentinel;
 *                           baseline path → block; non-append write to a ledger → block; other slice dirty → block.
 *   feedback tool_result    format/lint/file-length findings appended to the result; never blocks.
 *   gate     agent_settled  dirty tree must pass format, lint, typecheck, test, boundaries, file length, diff size;
 *                           failure → pi.sendUserMessage forces another turn. Three identical failures in a row →
 *                           notify and stop re-triggering (loop guard; the human decides).
 * Pi extension API: https://github.com/earendil-works/pi/blob/main/packages/coding-agent/docs/extensions.md
 */
import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";
import { execSync } from "node:child_process";
import { existsSync, readFileSync, writeFileSync } from "node:fs";
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
const dirtySlices = (b: Budgets, root: string): Set<string> =>
  new Set(sh("git status --porcelain", root).out.split("\n").filter(Boolean).map((l) => sliceOf(b, l.slice(3).trim())).filter((s): s is string => s !== null));

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
    if (target) { const dirty = dirtySlices(b, root); if (dirty.size && !dirty.has(target)) return { block: true, reason: `Slice fence: ${target} is not the active slice (dirty: ${[...dirty].join(", ")}). One slice per session: commit or stash first, or propose a separate task.` }; }
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
    if (!sh("git status --porcelain", root).out.trim()) return; // clean tree: nothing to gate
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
