## Goal

Speed up local dev build times on this (macOS) dev machine. Every sub-line must be proven with
a before/after timing comparison, not just "should be faster."

## Design

- Root `Cargo.toml` has no `[profile.*]` table; a prior contributor's comment in
  `apps/desktop/src-tauri/Cargo.toml:804-806` already flags this as deferred.
- `.cargo/config.toml` only has linker config for cross-compile targets (zig+lld for
  Linux/Windows); no native macOS (`aarch64-apple-darwin`) linker override exists.
- `mold` is Linux-only — doesn't apply on this macOS dev machine, dropped from scope.
- Heavy deps in the workspace: `rusqlite[bundled]`, `iroh`, `loro`, `tauri`, tokio/tonic/axum
  stack, across 16 workspace crates.
- `cargo check` needs no config change (rust-analyzer already runs it in the background) — not
  filed as a backlog item.

## Benchmark method

Run before any change (baseline), then again after each sub-line's change, and append the
numbers below so the comparison lives in the backlog, not just a chat transcript.

```sh
cargo clean -p txtodo-daemon
time cargo build -p txtodo-daemon        # clean-build baseline
touch crates/txtodo-daemon/src/main.rs
time cargo build -p txtodo-daemon        # incremental baseline
```

## Baseline

Measured 2026-09-21, `cargo build -p txtodo-daemon`, M-series macOS, warm `~/.cargo` cache:
- Clean (`cargo clean -p txtodo-daemon` first): **58.97s real** (53.40 user, 3.45 sys)
- Incremental (`touch crates/txtodo-daemon/src/main.rs`, single-crate recompile): **1.74s real**

## As built

**`profile.dev: debug = "line-tables-only"`** (2026-09-21):
- A profile edit invalidates cargo's fingerprint for the whole workspace, so the *first* build
  after adding it force-rebuilds every crate from scratch regardless of which one profile knob
  changed — 203.64s here, not comparable to the single-crate 58.97s baseline. This is a one-time
  cost inherent to touching `[profile.*]` at all, not something any one of these three sub-lines
  can avoid; it'll happen again once for sub-line 2 too.
- The number that actually isolates this change: single-crate incremental rebuild (touch
  `main.rs`, rebuild) — **1.73s**, vs. **1.74s** baseline. Flat, no measurable win. Expected:
  `line-tables-only` cuts debug-info *volume* per compile unit, which shows up on builds with
  heavy generic/monomorphized code or on a true full-workspace build, not a one-file touch of a
  small binary crate.
- Backtrace check: a standalone `rustc -C debuginfo=line-tables-only` panic still reports
  `panic_check.rs:2:5` under `RUST_BACKTRACE=1` — file:line survives, kept per acceptance
  criterion.
- Kept anyway: it's free (no downside found) and the debug-info reduction should compound on a
  true clean build even though this pass didn't isolate that number cleanly.

**`profile.dev.package."*": opt-level = 3`** (2026-09-21) — tried, reverted:
- One-time full-dep-graph rebuild (deps optimized): **261.63s**, vs. **203.64s** for the same
  full-dep-graph rebuild under sub-line 1 alone (deps at default opt-level 0). ~58s more
  expensive, exactly the tradeoff the acceptance criterion flagged as a real possibility.
- Single-crate incremental (touch `main.rs`, rebuild): **1.81s**, vs. **1.74s** baseline. Flat —
  within noise, no measurable win.
- Why it's flat: this override only changes how *dependency* crates compile; once they're built
  once, an incremental touch-one-file loop never recompiles them again regardless of their
  opt-level. The real payoff of "optimize deps once" is the *runtime* speed of the resulting
  debug binary (rusqlite/iroh/loro's hot paths), not compile time — which isn't what this backlog
  item is about (build speed). Given it only adds cost here with no compile-time win, reverted
  from `Cargo.toml` rather than kept on a "might help someday" basis.

**Native macOS linker (`-fuse-ld=lld`)** (2026-09-21) — declined without installing:
- `lld`/`ld64.lld` are not installed on this machine (`which` finds neither), and Homebrew's
  `llvm` keg isn't installed either — trying this means `brew install llvm` first, a multi-GB,
  invasive local-environment change, just to measure a linker swap of uncertain payoff.
- `xcrun --find ld` resolves to `/Library/Developer/CommandLineTools/usr/bin/ld`, reporting
  `version 1267` (`sw_vers`: macOS 26.6.2) — this is Apple's modern "new" linker (the ld64
  successor made default from Xcode 15 on), not the old classic linker `lld` would credibly beat.
  `mold` was already out of scope (Linux-only).
- Given the other two sub-lines already showed the compile-time upside here is marginal-to-flat,
  a multi-GB install to chase a linker swap against an already-modern default isn't worth it.
  Declined; no config added to `.cargo/config.toml`.

## Outcome

Net result of this pass: one small, free win kept (`debug = "line-tables-only"`, doesn't move
this machine's numbers but is a real, harmless reduction in debug-info volume with backtraces
still intact); two tried-and-declined, both undone, both because they don't move *compile* time
on this workspace despite being textbook advice — the "optimize deps"/"faster linker" tricks
mostly pay off in runtime speed or on Linux, not in this macOS + already-cached-deps dev loop.
Honest gap: none of the three sub-lines produced a measurable local build-speed win worth
writing home about; the backlog stays a record of what was tried and why it didn't, not a story
where every idea panned out.
