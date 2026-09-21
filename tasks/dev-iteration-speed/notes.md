## Goal

Follow-up to [[rust-build-speed]]: that pass found little local compile-time win on this macOS
machine. This one asks a broader "what else speeds up iteration" question and tries the two
answers with a real payoff: cargo-nextest (test speed) and sccache (conditional build speed).

## As built

**cargo-nextest** — adopted as the default local test runner:
- `crates/txtodo-core` (69 tests): `cargo nextest run` **1.33s** vs `cargo test` **13.46s**
  (both with test binaries pre-built) — ~10x.
- No doc-tests in this workspace to lose by switching (only one `toml`-tagged fenced block in
  `crates/txtodo-daemon/src/layout_file.rs`, not Rust-tagged so rustdoc never ran it as a test
  anyway) — nextest's lack of doc-test support is a non-issue here.
- `justfile`'s `test:` recipe and `.claude/budgets.json`'s `commands.test` both now read
  `cargo nextest run --workspace`, kept in lockstep (the drift audit the justfile's header
  comment refers to).
- Full-workspace proof: `just test` — **1211 tests passed, 91.44s**, no compatibility gaps
  (4 tests over the 60s slow-test threshold, all pre-existing real-daemon/pairing/crdt-sim
  tests, not new failures).
- **Deliberately not touched: `.github/workflows/ci.yml`**. CI's test steps are hand-sharded
  (OS-specific excludes, a separate `#[ignore]`d real-txtodod job, a separate Linux-only daemon
  job) for reasons documented inline there — converting that to nextest's `-E` filterset syntax
  is real surgery on shared CI infra, a bigger, separate decision than this pass's scope.

**sccache** — added as an opt-in `just cold-build` recipe, not the default build path:
- Repeated clean build of `txtodo-daemon` (simulates a fresh worktree rebuilding dependency
  versions already built elsewhere on this machine): **4.93s** vs **58.97s** baseline, 100%
  cache hit rate — ~12x.
- Requires `CARGO_INCREMENTAL=0`: sccache cannot cache incremental compilation artifacts at
  all. Tested with incremental left on (cargo's default) first: 0% hit rate, 6 non-cacheable
  calls, and the build was *slower* than no wrapper (75.67s vs 58.97s) — pure overhead, no
  upside. This is why it isn't wired into `.cargo/config.toml`'s default `[build]` — that would
  silently regress the normal edit-one-file loop, which relies on incremental compilation, for
  every local build, all the time.
- `just cold-build [ARGS]` (defaults to no args = `cargo build`; pass `-p txtodo-daemon` etc.)
  starts the sccache server if needed and runs with `RUSTC_WRAPPER=sccache
  CARGO_INCREMENTAL=0`. Use when starting fresh in a new worktree/branch, not for the normal
  active-editing loop.

## Outcome

nextest is an unconditional win, adopted as the default. sccache is real but conditional — kept
as an explicit opt-in rather than a silent default, the same "prove it, don't assume it" bar
[[rust-build-speed]] set.
