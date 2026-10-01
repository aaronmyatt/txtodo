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
- Fast-set coverage was estimated at 62-66% against CI's 72.48%. Measured since: 74.38% against
  81.57% (see As built).

## As built (2026-10-01)

Baseline, measured with another session's cargo running (load avg 15-30):
- The whole default nextest set ran 1,653 tests in 89.8 s wall, 608 s summed. 103 tests take
  ≥ 0.5 s and 71 take ≥ 1 s.
  - Biggest: daemon-launch `ensure_daemon` + `upgrade` (5 tests, ~67 s each: they start real
    daemons and run a nested cargo build), `crdt::sim twenty_fixed_seeds_converge` (87 s),
    `cli::pairing` (18 s).
  - The daemon lib's 494 tests sum to 26.5 s (max 2.6 s). sync: 191 tests, 7.2 s.
  - Slow but still on the fast side of the e2e line: `daemon::mutation_placement` (one test,
    4-5 s), `crdt::sim the_same_seed_twice...` (7.5 s instrumented), `cli::todosh_parity` (3.6 s).
- Line coverage of the fast set (no e2e candidates, no 87 s sim test): **74.38%**. CI, every test:
  **81.57%** (run 36822040427, 2026-10-01). budgets.json's 72 is stale (set 2026-09-17).
  - The fast set misses 3,328 lines that CI covers: cli 1,319, tui 827, daemon 583, desktop 251,
    daemon-launch 138, mcp 128. Nearly all of it is client code that talks to a real daemon
    (`cli/src/client.rs`, `tui/src/app_*.rs`, `desktop/src/daemon.rs`, `daemon/src/main.rs`).
  - Getting that back without a real daemon needs a fake gRPC server in those crates' tests
    (mcp already has `fake_backend`). Not planned yet.

Fast gate (`just fast`, `.claude/scripts/fast-gate.mjs`), same load:
- No crate changed: 30-150 ms. Desktop-only change: 0.47 s (vitest related 0.43 s).
- txtodo-store, warm, nothing rebuilt: 0.7-1.3 s.
- txtodo-store, after a real edit to one src file: **9.9 s**. clippy 0.5 s; nextest 9.9 s, most
  of it rebuilding and linking the 13 store test binaries. This is what the it/e2e merge targets.
- Cold `target/lint` (first clippy run per crate): 11 s once.
- A planted fmt error, an unused variable and a failing test each fail it (exit 1, tail shown).

Done:
- `check-file-length.sh` 10.7 → 0.36 s, `check-boundaries.sh` 2.6 → 0.4 s (same output).
- One entrypoint, `budgets.json.commands.fast`: pre-commit runs it with `--staged`, pre-push with
  the pushed range, gate.sh and the Pi twin minus other sessions' leased crates. Workspace clippy
  and `cargo check` left the local hooks; CI keeps workspace clippy.
- All steps run side by side. The total is the slowest step (nextest), not the sum.
- nextest `fast` profile: inherits the default filter (now also skips `slow_*`), flags SLOW at
  1 s, kills a test at 10 s.
- feedback.sh's clippy uses `target/lint` with the gate's args, so each edit warms the gate.
  Its file-length now checks only the edited file.

Still broken / open:
- The 5 s budget only holds today for small crates with nothing to rebuild. Any real edit
  relinks every test binary of the crate. Next: the per-crate it/e2e merges.
- `cargo fmt --all --check` measured 19.5 s once and 0.8 s once: the 19.5 s was I/O contention,
  not rustfmt.
