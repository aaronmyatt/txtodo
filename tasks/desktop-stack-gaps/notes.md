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
