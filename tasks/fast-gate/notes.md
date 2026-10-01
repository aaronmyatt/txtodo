## Goal

One local check after every change, before commit and push, in under 5 s. Anything slower runs
in CI. Asked 2026-10-01.

Decided with the human the same day:
- The gate covers the crates changed in the diff only. Crates that depend on them are CI's job.
- Each crate's `tests/*.rs` merges into two binaries: `tests/it/` (fast, in-process) and
  `tests/e2e/` (starts `txtodod`, uses the network/mDNS/relay, or runs a nested cargo build).
  One crate per session, under the slice fence.
- Playwright stays manual. ci.yml stays cost-frozen for it.

## Before (measured 2026-10-01, load avg ~9 from another session's cargo)

- `cargo fmt --all --check` 19.5 s. `rustfmt --check` on the files alone: 1.4 s CPU.
- `check-file-length.sh` 22.3 s. It forks `echo | grep` and `wc` once per file, over 756 files.
  One `xargs wc -l | awk` pass takes 0.03 s.
- `check-assertions.sh` 14.8 s, advisory only (it always exits 0).
- `check-boundaries.sh` 3.2 s. `check-version-sync.sh` 0.25 s.
- vitest: 2.0 s wall for 180 tests.
- 134 `tests/*.rs` files means 134 test binaries, and each one links the whole crate.
- `target/` is 274 GB (163 GB incremental, 93 GB / 1.2M files in deps).

## Design

- `.claude/scripts/fast-gate.mjs` is the one entrypoint. pre-commit, pre-push, gate.sh, the Pi
  twin and `just fast` all call it (as `budgets.json.commands.fast`).
  - Changed files come from `git diff --name-only <base>` plus untracked files. A path prefix
    maps each file to a crate; no `cargo metadata`.
  - Cheap steps go first, and only for what changed: rustfmt on changed `.rs` files,
    file-length, boundaries (only when a Cargo.toml changed), version-sync, specs-mirror.
  - Then, in parallel: clippy `-p <changed>` in `target/lint`, nextest `-p <changed> --profile
    fast`, and vitest when apps/desktop changed.
  - clippy gets its own target dir because cargo locks the target dir. Clippy only checks and
    nextest needs linked binaries, so they cannot share output anyway.
  - It prints each step's ms and the total. Over `fastGateMs` it warns; it fails only with
    `--strict`. Every run is logged to `.git/fast-gate-times.tsv`.
- `cargo check` is dropped from the gate: clippy is a superset (ci.yml already says so).
- The e2e test target is `[[test]] name = "e2e" test = false`, so local builds and
  `clippy --all-targets` skip it, and CI asks for it with `--test e2e`. I picked this over
  `required-features`, because a feature changes the lib's build hash and builds dependents twice.
- Coverage moves to `cargo llvm-cov nextest`: once merged, tests that call `set_var` share a
  process under plain `cargo test`.

## Known limits (before any of this lands)

- The daemon will likely stay over 5 s. Its 431 unit tests add up to ~60 s, which is ~5 s on
  14 cores before any build.
- An edit to txtodo-core still makes rustc rebuild its own crate. Dependents are not
  rebuilt locally; CI covers them.
- Fast-set coverage is an estimate until measured: 62-66% against CI's 72.48%. e2e alone reaches
  `main.rs`, LAN/relay/pairing transport, crash recovery and single-instance.
