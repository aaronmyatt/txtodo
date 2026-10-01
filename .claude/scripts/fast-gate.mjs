#!/usr/bin/env node
// txtodo fast gate (task fast-gate): the one local check run after every change and before every
// commit and push, budgeted at budgets.json.fastGateMs (5 s). It checks only what the diff touched:
// the crates whose files changed (not their dependents: CI covers those), the changed .rs files'
// format and length, and apps/desktop's vitest when the frontend changed. Slow tests (real
// txtodod, network, mDNS, relay) live in each crate's tests/e2e and in CI, never here.
//
// Callers: .githooks/pre-commit, .githooks/pre-push, .claude/hooks/gate.sh, the Pi twin
// (.pi/extensions/guardrails/index.ts) and `just fast`, all through budgets.json.commands.fast.
//
// Usage: fast-gate.mjs [--base <rev>] [--head <rev> | --staged] [--exclude-crates "<a> <b>"] [--strict]
//   --base            diff against this rev (default HEAD: staged + unstaged + untracked files)
//   --head            diff --base..--head instead of the working tree (pre-push: the pushed sha)
//   --staged          only what is staged (pre-commit): another session's unstaged crate is not
//                     this commit's. `git commit -- <paths>` stages into a temporary index that
//                     git hands the hook as GIT_INDEX_FILE, so this sees exactly those paths.
//   --exclude-crates  crates leased by another agent session: their mid-edit state is not ours
//   --strict          fail, not just warn, when the run is over budget
// Exit 0 when every step passed; 1 when any step failed (or over budget with --strict).
import { spawn, spawnSync } from "node:child_process";
import { appendFileSync, existsSync, readFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { performance } from "node:perf_hooks";
import { parseArgs } from "node:util";

// https://nodejs.org/api/util.html#utilparseargsconfig
const { values: args } = parseArgs({
  options: {
    base: { type: "string", default: "HEAD" },
    head: { type: "string" },
    staged: { type: "boolean", default: false },
    "exclude-crates": { type: "string", default: "" },
    strict: { type: "boolean", default: false },
  },
});

const git = (...a) => {
  // https://nodejs.org/api/child_process.html#child_processspawnsynccommand-args-options
  const r = spawnSync("git", a, { encoding: "utf8" });
  if (r.status !== 0) throw new Error(`git ${a.join(" ")}: ${r.stderr.trim()}`);
  return r.stdout;
};
const ROOT = git("rev-parse", "--show-toplevel").trim();
process.chdir(ROOT);
const budgets = JSON.parse(readFileSync(".claude/budgets.json", "utf8"));
const BUDGET_MS = budgets.fastGateMs ?? 5000;
const t0 = performance.now();

// --- What changed -------------------------------------------------------------------------------
// `git diff --name-only <base>` compares the working tree (staged and unstaged) with <base>;
// `--cached` compares the index; with --head it compares two commits. Untracked files only count
// for a working-tree run. https://git-scm.com/docs/git-diff
const lines = (s) => s.split("\n").filter(Boolean);
const changed = args.head
  ? lines(git("diff", "--name-only", args.base, args.head))
  : args.staged
    ? lines(git("diff", "--cached", "--name-only", args.base))
    : [...lines(git("diff", "--name-only", args.base)), ...lines(git("ls-files", "--others", "--exclude-standard"))];
const exists = (f) => existsSync(f);

// Path prefix → Cargo package. Every crates/<dir> is named <dir> (checked 2026-10-01); the two
// members outside crates/ are listed by hand. crates/txtodo-core/fuzz is not a workspace member.
const crateOf = (f) => {
  if (f.startsWith("crates/txtodo-core/fuzz/")) return null;
  const m = /^crates\/([^/]+)\//.exec(f);
  if (m) return m[1];
  if (f.startsWith("apps/desktop/src-tauri/")) return "desktop";
  if (f.startsWith("relay/")) return "relay";
  return null;
};
// Workspace-wide inputs: a change here can break any crate, which is CI's job, not 5 s worth.
const WORKSPACE_WIDE = /^(Cargo\.(toml|lock)|\.cargo\/|clippy\.toml|rustfmt\.toml|rust-toolchain\.toml)/;
const glob = (g) => new RegExp("^" + g.replace(/[.+^${}()|[\]\\]/g, "\\$&").replace(/\*\*/g, "\0").replace(/\*/g, "[^/]*").replace(/\0/g, ".*") + "$");
const generated = (budgets.generatedPaths || []).map(glob);

const excluded = new Set(args["exclude-crates"].split(/\s+/).filter(Boolean));
const crates = [...new Set(changed.map(crateOf).filter((c) => c && !excluded.has(c)))].sort();
const rsFiles = changed.filter(
  (f) => f.endsWith(".rs") && exists(f) && crateOf(f) && !excluded.has(crateOf(f)) && !generated.some((r) => r.test(f)),
);
const desktopFiles = changed.filter((f) => f.startsWith("apps/desktop/") && !f.startsWith("apps/desktop/src-tauri/") && exists(f));
const wide = changed.filter((f) => WORKSPACE_WIDE.test(f));
const any = (re) => changed.some((f) => re.test(f));

// --- Steps --------------------------------------------------------------------------------------
// Each step is { name, cmd, argv, env?, cwd? }. All of them run side by side: the total is the
// slowest step (the nextest build), not the sum. 14 cores leave room for the cheap ones.
const steps = [];
if (rsFiles.length) {
  // rustfmt on the changed files alone, not `cargo fmt --all` over the workspace. A file's
  // out-of-line `mod` children are checked with it. https://github.com/rust-lang/rustfmt#running
  steps.push({ name: "rustfmt", cmd: "rustfmt", argv: ["--edition", "2024", "--check", ...rsFiles] });
  steps.push({ name: "file-length", cmd: ".claude/scripts/check-file-length.sh", argv: rsFiles });
}
if (any(/(^|\/)Cargo\.toml$|^\.claude\/budgets\.json$/)) steps.push({ name: "boundaries", cmd: ".claude/scripts/check-boundaries.sh", argv: [] });
if (any(/^(Cargo\.toml|apps\/desktop\/src-tauri\/tauri\.conf\.json|apps\/desktop\/package\.json)$/)) {
  steps.push({ name: "version-sync", cmd: "scripts/check-version-sync.sh", argv: [] });
}
if (any(/^(specs\/|txtodo-design\.md$|txtodo-implementation-plan\.md$)/)) steps.push({ name: "specs-mirror", cmd: ".claude/scripts/check-specs-mirror.sh", argv: [] });

if (crates.length) {
  const p = crates.flatMap((c) => ["-p", c]);
  // clippy builds into its own target dir so it can run beside nextest: cargo holds a lock on the
  // whole target dir for a build, and the two cannot share output anyway (clippy only checks,
  // nextest needs linked binaries). feedback.sh uses the same dir, so each edit warms this step.
  // `--all-targets` leaves out tests/e2e: that target is `test = false` (CI asks for it by name).
  // https://doc.rust-lang.org/cargo/reference/environment-variables.html#environment-variables-cargo-reads
  steps.push({
    name: "clippy",
    cmd: "cargo",
    argv: ["clippy", "--quiet", ...p, "--all-targets", "--", "-D", "warnings"],
    env: { CARGO_TARGET_DIR: join(ROOT, "target/lint") },
  });
  // The `fast` nextest profile: no slow_* tests, SLOW flagged at 1 s, a hung test killed at 10 s.
  // PROPTEST_CASES caps property tests that read it (proptest's default config does).
  // https://nexte.st/docs/running/#--no-tests  https://docs.rs/proptest/latest/proptest/test_runner/struct.Config.html
  steps.push({
    name: "nextest",
    cmd: "cargo",
    argv: ["nextest", "run", "--cargo-quiet", ...p, "--profile", "fast", "--no-tests=pass"],
    env: { PROPTEST_CASES: "256" },
  });
}
if (desktopFiles.length && existsSync("apps/desktop/node_modules/.bin/vitest")) {
  // `vitest related` runs only the tests that import a changed file (a changed test runs itself).
  // A config or package change runs them all. https://vitest.dev/guide/cli.html#vitest-related
  const rel = desktopFiles.map((f) => f.slice("apps/desktop/".length));
  const all = rel.some((f) => /^(package(-lock)?\.json|vite(st)?\.config\.ts|tsconfig\.json)$/.test(f));
  steps.push({
    name: "vitest",
    cmd: "node_modules/.bin/vitest",
    argv: all ? ["run", "--reporter=dot"] : ["related", "--run", "--reporter=dot", "--passWithNoTests", ...rel],
    cwd: "apps/desktop",
  });
}

// --- Run ----------------------------------------------------------------------------------------
// https://nodejs.org/api/child_process.html#child_processspawncommand-args-options
const run = (s) =>
  new Promise((done) => {
    const start = performance.now();
    const child = spawn(s.cmd, s.argv, { cwd: s.cwd ?? ROOT, env: { ...process.env, ...s.env } });
    let out = "";
    child.stdout.on("data", (d) => (out += d));
    child.stderr.on("data", (d) => (out += d));
    child.on("error", (e) => done({ ...s, ok: false, ms: performance.now() - start, out: String(e) }));
    child.on("close", (code) => done({ ...s, ok: code === 0, ms: performance.now() - start, out }));
  });

const results = await Promise.all(steps.map(run));

// --- Report -------------------------------------------------------------------------------------
const total = performance.now() - t0;
const ms = (n) => `${Math.round(n)} ms`.padStart(9);
const scope = crates.length ? crates.join(" ") : "no crates";
console.log(`fast-gate: ${scope}${desktopFiles.length ? " + desktop" : ""} (${changed.length} changed files vs ${args.base}${args.head ? ".." + args.head : args.staged ? ", staged" : ""})`);
for (const r of results) {
  console.log(`  ${r.ok ? "ok  " : "FAIL"} ${r.name.padEnd(13)}${ms(r.ms)}`);
  // The tail is enough to act on; the full output is one re-run of the step's command away.
  if (!r.ok) console.log(r.out.trim().split("\n").slice(-40).map((l) => "       " + l).join("\n"));
  else if (r.name === "nextest" && /\bSLOW\b/.test(r.out)) {
    console.log(r.out.split("\n").filter((l) => /\bSLOW\b/.test(l)).map((l) => "       " + l.trim()).join("\n"));
  }
}
if (wide.length) console.log(`  note: workspace-wide files changed (${wide.join(", ")}): CI checks every crate`);
const over = total > BUDGET_MS;
console.log(`  total${ms(total).padStart(22)}${over ? `  — over the ${BUDGET_MS} ms budget` : ""}`);

const failed = results.filter((r) => !r.ok).map((r) => r.name);
// One line per run, for real p50/p95 later: ISO time, total ms, pass/fail, crates, step=ms...
// `--git-dir` is per worktree, so each worktree keeps its own log.
try {
  const row = [new Date().toISOString(), Math.round(total), failed.length ? "fail" : "pass", crates.join(",") || "-", ...results.map((r) => `${r.name}=${Math.round(r.ms)}`)];
  appendFileSync(resolve(git("rev-parse", "--git-dir").trim(), "fast-gate-times.tsv"), row.join("\t") + "\n");
} catch {
  // A read-only .git must never fail the gate.
}
process.exit(failed.length || (args.strict && over) ? 1 : 0);
