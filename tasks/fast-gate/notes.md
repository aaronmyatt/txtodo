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

## Merge recipe (from the store and daemon-launch merges, 2026-10-01)

- Fast files → `tests/it/main.rs` (`mod foo;` each). Slow files (start txtodod, network, mDNS,
  relay, nested cargo build) → `tests/e2e/main.rs` + `[[test]] name = "e2e" path =
  "tests/e2e/main.rs" test = false` in the crate's Cargo.toml.
- A file that calls `std::env::set_var`/`remove_var` stays its own `tests/*.rs` binary: plain
  `cargo test` runs one binary's tests as threads of one process, and these files' own docs say
  they rely on being alone (daemon-launch `autostart`, `service_disabled`; daemon `debug_hooks`).
- Per moved file: drop `mod support;`, `use support::` → `use crate::support::`, then
  `rustfmt` (the import order changes). `include_str!("../x")` → `"../../x"`.
- Cargo drops a named `--test e2e` when `--tests` or `--all-targets` is also given (checked: 0 e2e
  tests listed). So CI has separate e2e steps, and the gate chains a second clippy for e2e.
- `--workspace --test e2e` works as long as one selected package has an e2e target; a lone
  package without one errors.
- When txtodo-daemon gets its e2e target, the CI daemon job needs a second command:
  `cargo nextest run -p txtodo-daemon --test e2e` (the check job excludes the daemon).
- Measured: store after a src edit 10.0 s → 2.9 s (same load). daemon-launch: its 5 real-daemon
  tests (67 s each under load in the default set) left the local run; gate 3.7 s after an edit.
- Stale pointer left for the cli session (crate leased elsewhere today):
  `crates/txtodo-cli/tests/daemon_autostart.rs:122` names `daemon-launch/tests/upgrade.rs`.

## Slow tags, proptest cases, bin test=false (2026-10-02)

Measured `cargo nextest run --workspace --lib --bins` at load ~2.5: 1,308 tests, 7.5 s wall.
26 took ≥ 0.4 s. Rule used: ≥ 0.5 s in that run gets a `slow_` name, nothing else.
- daemon 17 (bundle and bundle_crypto Argon2, the 5 s rejoin, the real-watcher catalog test, LAN
  session resend/fairness, keystore). Lib run 5.4 → 1.5 s wall.
- sync 5 (real mDNS, the holepunch gate wait, two Argon2 keystore files). Lib 0.6 s.
- cli 1 (`plan_audit` tagged, 2.7 s, in the bin target). tui 1 (the 0.6 s ready timeout).
- Left on purpose: 0.48-0.49 s tests (sync `wrong_passphrase_is_refused_not_corrupted`, daemon
  `stuck_sync_session`). A sibling of two tagged keystore tests stays untagged at 0.48 s.
- RATCHET.md's two pointers follow the renames. Old names in older task notes are left as history.
- `differential.rs`: `with_cases(10_000)` ignored `PROPTEST_CASES`. Now that variable wins, then
  `CI=true` gives 10 000, else 1 000. 0.64 → 0.07 s locally. ci.yml needs no change.
- tui and mcp `[[bin]] test = false`: neither main.rs has tests. tests/*.rs still get the binary.

Still slow: the gate on daemon is 61 s, cli 32 s, all from `tests/*.rs` binaries. That is the
per-crate merge lines, not this one.

Optional, not done: most of the Argon2 cost is a debug build. `[profile.dev.package.argon2]
opt-level = 3` in the root Cargo.toml (frozen for agents) would likely bring ~12 of these tests
back under 0.5 s, so they could lose the `slow_` name and rejoin the fast set.

## Rejected: cargo-hakari workspace-hack (2026-10-01)

Tried: one feature set for every shared dependency, via a hakari workspace-hack crate wired as a
macOS-only dev-dependency of every member (test and clippy builds only; release, no-std, wasm and
CI untouched). It did unify: crates built 2+ ways across `-p` selections went 100 → 0 (154 extra
copies, tokio 8 ways, serde_core 7, syn 6).

It made builds slower. Same gate commands (clippy + nextest --no-run) after a one-line edit per
crate, mean of 3 rounds, separate worktree, load ~15-19 before vs ~10 after:
- store 2.7 → 4.9 s, daemon-launch 2.5 → 5.1 s, telemetry 2.3 → 4.3 s, sync 3.8 → 4.2 s,
  mcp 7.3 → 17.8 s, daemon 31.6 → 75.5 s, `--workspace` build 69 → 169 s.
- Median per edit 3.4 → 4.9 s.

Why:
- The union carries the desktop's choices into every crate. `tauri-macros` turns on
  `proc-macro2/span-locations`, which slows every derive (serde, tracing, prost, clap) in every
  rebuild; before, only desktop builds paid it.
- Extra copies never cost time per edit: every copy stays on disk, so switching selections does
  not rebuild them. An edit costs the edited crate's rebuild and its test-binary links, which the
  it/e2e merge cuts (store 10.0 → 2.9 s) and unification cannot.

Not settled: the hakari run reused the first run's target dir (351k files in deps/), and build
times rose every round in both runs (daemon 24 → 40 s before, 65 → 86 s after). Something
accumulates in target/, possibly that directory. A periodic `cargo clean`/cargo-sweep may be a
speed fix as well as a disk fix; not measured.

Disk stays the job of `cargo clean` or cargo-sweep (https://github.com/holmgr/cargo-sweep).
