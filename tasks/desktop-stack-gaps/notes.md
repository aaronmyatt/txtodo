# desktop-stack-gaps

Found while writing `.claude/stack.md`'s "second stack" mapping for `apps/desktop` (Tauri 2 +
SvelteKit), not part of that task's own scope, so flagged here instead of fixed inline.

Two independent gaps, different risk:

- **Two Rust-tier scripts hardcode `crates/*`.** `check-file-length.sh`'s `find` and
  `check-boundaries.sh`'s `for manifest in ... crates/*/Cargo.toml` both never reach
  `apps/desktop/src-tauri/Cargo.toml`, even though it's a real member of the root workspace
  (`Cargo.toml`'s `members`). Mechanical: widening a glob to a path that's already a workspace
  member, not a new rule or a new architecture. Safe to do without a human call.
- **`apps/desktop` has zero CI coverage.** No `.github/workflows/ci.yml` step runs `svelte-check`,
  `vitest`, `playwright`, or even builds the app. Fixing this for real means a new CI job: pick a
  Node version, decide whether Playwright's browser download belongs in CI (heavy, and this repo's
  own `ci.yml` is frozen), decide whether visual/e2e specs run on every PR or a slower nightly. Real
  design questions, not mine to decide alone — `@human`.

## As built (2026-09-17, agent)

The mechanical gap is closed. `check-boundaries.sh` now also loops over
`apps/desktop/src-tauri/Cargo.toml`; its `budgets.json.slices.allowedDeps` lookup key is derived
from the manifest directory's basename, which for this crate is `src-tauri` (not `desktop`, its
Cargo.toml package name) — a real naming trap, documented inline in both the script and
`budgets.json`. `check-file-length.sh`'s `find` now includes the same directory.

Turning these on immediately found something real: `apps/desktop/src-tauri/src/commands.rs` was
already 462 lines *before* this session touched it at all (462 on the parent commit, 471 after this
session's own `skill_hint` addition) — a genuine, pre-existing budget violation that was invisible
only because nothing ever scanned this file. Fixed, not just flagged: split `ui_log` and its five
level-fanout helpers (plus their one test) into a new `commands_ui_log.rs`, the same
split-for-the-file-budget pattern the sibling `commands_notes.rs`/`commands_pairing.rs`/etc. already
use. Result: `commands.rs` 352 lines, `commands_ui_log.rs` 125. Both scripts exit 0 now, and
`cargo check/test/clippy/fmt --workspace --all-targets` all stayed green through the split.

The CI-wiring line stays open and `@human`-tagged, per the design questions above — nothing about
those changed by fixing the mechanical half.

## As built (2026-09-18, agent) — CI-coverage half closed

Answered the three design questions this task's own `@human` tag flagged, per the overnight task
brief that authorized this build (not re-litigated further):

- **Node version**: 22, not the "else Node 20" fallback this task's own brief suggested —
  `apps/desktop/package.json` has no `engines` field, but `.github/workflows/release.yml`'s
  `build-desktop` job already pins Node 22 for this same app; matching that existing convention
  beat introducing a second Node version for one more job.
- **Playwright's browser download**: does NOT belong in the required PR gate. New
  `.github/workflows/desktop-e2e-nightly.yml` (`on: schedule` daily at 07:00 UTC + `workflow_
  dispatch`) runs the full suite (`npm run test:e2e` — both the six functional specs and the
  light/dark visual-regression projects, `playwright.config.ts`'s own `projects` list) on that
  separate cadence instead. No pre-existing nightly/scheduled workflow existed to extend, so this
  is a new file, not an addition to one.
- **Runner OS**: `ubuntu-latest` for the new required `desktop` job (cheaper; svelte-check/
  vitest/build need no real macOS/Tauri runtime), reusing the same Tauri Linux system-dependency
  apt-get list the `check` job already has (webkit2gtk/gtk/appindicator/etc — duplicated inline,
  not factored into a composite step, matching this file's existing no-composite-actions style).
  The nightly e2e workflow also runs on `ubuntu-latest` for the same reason, plus `npx playwright
  install --with-deps` for the browsers themselves.

**New required job**: `ci.yml`'s `desktop` job runs `npm ci`, `npm run check` (svelte-check),
`npx vitest run`, `npm run build`, in that order. No aggregating "all jobs must pass" gate job
exists anywhere in this repo (`ci.yml` has no such job, and there is no branch-protection-as-code
file — `.github/` has only the two workflow files) for this new job to be added to; GitHub's own
branch-protection required-checks list (configured outside this repo, in the GitHub UI/API) is
what would need `desktop` added to it by whoever administers that — flagged here since this repo
has no way to encode that itself.

**Verification — what's real vs not**: every command the new `desktop` job runs was actually
executed locally in this session against the real `apps/desktop` tree (after this session's own
tray/pin/sidecar changes): `npm run check` (0 errors), `npx vitest run` (113 tests passing),
`npm run build` (succeeds, static adapter output written). Both new workflow YAML files were
syntax-checked (`python3 -c "import yaml; yaml.safe_load(...)"`), and mirror this repo's own
existing job patterns closely. **What is NOT verified**: an actual GitHub Actions run of either
workflow — the ubuntu-latest apt-get package list, `actions/setup-node@v4`'s Node 22 resolution,
and (for the nightly workflow) `npx playwright install --with-deps` succeeding and the real e2e
suite passing against a runner-built `e2e_bridge`/`txtodod`, are only provably correct once a real
PR or a manual `workflow_dispatch` run exercises them. Flagging plainly per this task's own
instruction: this cannot be verified as green without a real CI run.
