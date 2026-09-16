# Converge relay's tracing init (root todo.txt `logging-relay-converge`, id:01M2P0RELAYCONVERGELOGS001)

## Goal

`relay/src/main.rs::init_tracing` is today plain-text to stdout via `.init()` (panics on double
init) under `RELAY_LOG` — the last divergent tracing idiom in the workspace, against every other
binary's JSON-rolling-file + pretty-stderr pair via `.try_init()` under `TXTODO_LOG`
(`txtodo_telemetry::init`, `crates/txtodo-telemetry/src/lib.rs`). Converge relay onto the same
*shape* (JSON rolling file, pretty stderr, non-panicking init), keeping `RELAY_LOG` working.

## Blocking discovery: relay cannot depend on `txtodo_telemetry`

The obvious move — add `txtodo-telemetry` as a dependency and call `txtodo_telemetry::init("relay",
...)` — is structurally impossible here, discovered by actually reading `relay/`'s own guardrails
before touching anything:

- `relay/tests/no_txtodo_deps.rs::manifest_has_no_txtodo_dependency` asserts `relay/Cargo.toml`
  never contains the substring `"txtodo-"`. It is not a style nit: the module doc states *"The
  relay must never depend on any `txtodo-*` crate: it is untrusted (design §4.6) and should be
  structurally incapable of importing the crypto that would let it read what it stores."* Adding
  `txtodo-telemetry` as a dependency fails this test on purpose, by design.
- `relay/Cargo.toml`'s own header comment repeats this: "Deliberately zero dependencies on this
  workspace's own crypto/parsing crates... The relay is untrusted... it must never be able to
  parse or decrypt what it stores."
- `.claude/budgets.json`'s `slices` map has **no entry for `relay`** at all — so there is no
  `allowedDeps` list to extend either; the boundary isn't policy-configurable, it's a hard test.

This is a real, load-bearing security invariant, not an oversight to route around. I do not have
standing to weaken or delete `no_txtodo_deps.rs` to make room for a dependency, and the task brief
itself says: *"If something needs a human decision you can't make..., stop, document it, leave
that piece undone rather than guessing."* Weakening the untrusted boundary is exactly that kind of
call — so this task does **not** touch `no_txtodo_deps.rs`, and does **not** add `txtodo-telemetry`
(or any `txtodo-*` crate) to `relay/Cargo.toml`.

**Resolution**: match `txtodo_telemetry::init`'s *shape* (JSON daily-rotated file + pretty stderr,
one shared `EnvFilter`, 7 files kept, non-panicking `.try_init()`) using the same underlying
third-party crates (`tracing-appender`, `tracing-subscriber`) directly in `relay/src/main.rs`,
without depending on the shared crate. This is duplication of a small, self-contained amount of
logic (rotating-file setup + prune, ~40 lines) — an accepted, documented tradeoff of relay's
zero-`txtodo-*`-deps invariant, not a drift the way the *pre-existing* `.init()`-to-stdout code
was a drift (that had no such invariant forcing it; it was just never converged).

**One further, related discovery — also forced by the same invariant**: `std::env::set_var` is an
`unsafe fn` as of this toolchain (confirmed: `rustc 1.95.0`, edition 2024), and
`relay/src/main.rs` carries `#![forbid(unsafe_code)]`. Even if relay *could* depend on
`txtodo_telemetry`, that crate's `init(service, logs_dir)` reads `TXTODO_LOG` directly from the
process environment with no way to inject a resolved filter string — so making `RELAY_LOG` alias
it would have required either `unsafe { std::env::set_var(...) }` (forbidden in this crate) or
extending `txtodo_telemetry`'s public API (out of this task's stated file scope: relay/,
`.claude/budgets.json` only if relay had a slice entry, `tasks/logging-relay-converge/`, and the
one root todo.txt line). The local-reimplementation resolution above sidesteps this too: relay
resolves its own filter string (`RELAY_LOG` then `TXTODO_LOG`) as a plain, safe, injectable-for-
testing function and builds its own `EnvFilter` from it directly — no env mutation anywhere.

## Design

`relay/src/main.rs` gains:

- `resolve_log_filter(env: &dyn Fn(&str) -> Option<String>) -> String` — pure, injectable exactly
  like `config.rs`'s own `Source`/`no_env` pattern (same file, same convention already
  established in this crate). Checks `RELAY_LOG` first, falls back to `TXTODO_LOG`, defaults to
  `"info"`.
- `build_filter() -> EnvFilter` — wraps `resolve_log_filter` with the real `std::env::var`, parses
  it via `EnvFilter::try_new`, falling back to `"info"` on a bad directive string (matches
  `txtodo_telemetry::build_filter`'s own fallback behavior).
- `prune_logs(logs_dir) -> io::Result<()>` — keeps the newest `LOG_KEEP_FILES` (= 7, matching
  `txtodo_telemetry::LOG_KEEP_FILES`) `relay.log.*` files by lexical filename sort, same algorithm
  as `txtodo_telemetry::prune` (not called directly — see above).
- `LogGuard` (local, private) — holds the `tracing_appender::non_blocking::WorkerGuard`; `main`
  holds it until the process exits, same discipline as `txtodo_telemetry::LogGuard`'s own doc.
- `init_tracing(data_dir: &Path) -> Option<LogGuard>` — creates `<data_dir>/logs`, prunes, builds
  the daily-rotating JSON writer (`tracing_appender::rolling::daily`) and a pretty stderr layer,
  installs both under one shared `EnvFilter` via `.try_init()` (never panics; a failure — e.g.
  double-init in a test — is logged to stderr and swallowed, matching `txtodo_telemetry::init`'s
  own "logging must never crash the binary" spirit even though the return type here is `Option`
  not `io::Result`, since `main.rs`'s own call site doesn't need to distinguish the failure
  reason).

`RELAY_LOG` vs `TXTODO_LOG` precedence, stated explicitly: **`RELAY_LOG` wins if set.** Rationale:
`RELAY_LOG` is relay's own, already-documented, already-deployed variable (`docs/relay.md`'s flags
table lineage, `deploy/systemd/relay.service`'s `EnvironmentFile`) — an operator who already set it
must see identical behavior after this change, so it cannot become a silent no-op. `TXTODO_LOG` is
the fallback so a relay instance that has never been specially configured behaves like every other
txtodo binary in the workspace by default.

### Where logs live

`init_tracing` is called from the new `run(config: Config)` (config now known), not from `main()`
before argument parsing — `main.rs` previously called `init_tracing()` as the very first line, before
`config::parse` runs, but the JSON file layer needs a real directory and the only directory this
binary ever owns is `--data-dir`/`RELAY_DATA_DIR`, which isn't resolved until parsing succeeds.
This mirrors `txtodo-daemon`'s own precedent (`prepare_and_announce`, called from `run(args)` after
`state_dir` is resolved, not from `main()` before `Args` parsing) — log init happens once the
binary knows where to put files, not before. The `--help` and argument-parse-failure paths are
unaffected: they already only ever used `println!`/`eprintln!` (the file's own
`#![allow(clippy::print_stderr, clippy::print_stdout)]` comment: "the binary's only human output
path... --help and a fatal config error have nowhere else to go") and still do — no regression,
since neither path reaches `run`.

Log files land at `<data_dir>/logs/relay.log.YYYY-MM-DD` — sibling to `relay.db` under the same
`--data-dir`/`RELAY_DATA_DIR` the operator already provides, the same "logs live next to the
binary's own state" placement every other converged binary uses (daemon: `state_dir/logs`; cli:
`dir/.txtodo/logs`).

### Deployment: stdout/file, resolved

`relay-kamal-deploy`'s own notes.md says explicitly: **"`relay/` is not part of this deployment.
Different server... Keep it off the droplet, or on loopback, until it has its own access story."**
The Kamal-deployed service is a *different* binary entirely (a stock `iroh-relay`), so Kamal's
config is irrelevant to this task. The relevant deploy artifact for *this* `relay/` binary is
`deploy/systemd/relay.service` — a user-scoped systemd unit with no stdout/stderr redirect, so
systemd's own default (capture into the user journal) applies, exactly like `txtodod.service`'s own
documented precedent: *"stdout/stderr land in the user journal by systemd's own default — the
daemon's own structured JSON logs are the durable record."* Converging relay the same way (JSON
file as the durable record, pretty stderr redundantly captured by the journal) needed no deploy
config change — the unit file already does the right thing by doing nothing special.

## Placement

All in `relay/src/main.rs` — no new files; the addition is small enough (~70 lines) that a new
module would cost more (a `mod` line, a file-length budget entry) than it saves. `relay/Cargo.toml`
gains `tracing-appender` (already a workspace-pinned dependency, `0.2`, used elsewhere in the
workspace — not a `txtodo-*` crate) and `tracing-subscriber`'s `json` feature (already has
`env-filter`). `docs/relay.md` gains a short "Logging" section (env vars, file location) — nothing
here was documented before. `deploy/systemd/relay.service`'s comment gets one added line noting
`RELAY_LOG`/`TXTODO_LOG` now also control a JSON file under `--data-dir/logs`, alongside the
existing journal capture.

## Edge cases

- **No payload/secret leakage**: nothing in this change adds a new log call — it only changes how
  existing `tracing::info!`/`tracing::warn!` calls in `http.rs`/`retention.rs`/`push.rs` etc. are
  formatted and where they land. Those call sites were already audited under the existing
  "ids, counts and hashes only" rule; unchanged here.
- **No `service` field on relay's JSON lines** (a real, deliberate gap from `txtodo_telemetry`'s
  shape): the field-stamping trick (`txtodo-telemetry/src/stamp.rs`) is itself ~100 lines of a
  byte-level `Write` wrapper — reimplementing it here for a single-process binary that is never
  colocated with other txtodo binaries' log files in the common (self-hosted, or historically
  intended standalone) deployment gains little, and duplicating that much logic on top of the
  rotating-file logic already being duplicated felt like the wrong trade. Flagged here, not
  silently dropped, in case the daemon-relay-colocated-logs use case ever needs it.
- **Double-init**: `init_tracing` uses `.try_init()` and swallows (logs to stderr) a failure —
  matters for `#[tokio::test]`-style relay tests that might exercise `run()` more than once in one
  process; previously `.init()` would have panicked.
- **`--retention-days`/`--max-blob-bytes` numeric parse failures, `--listen` bind failures**:
  unaffected — those still return `ExitCode::FAILURE` via the existing `eprintln!` paths, now
  simply also visible on stderr through the new pretty layer once logging is up (parse failures
  before `run()` still print via the pre-existing `Err(message)` arm in `main()`, untouched).

## Acceptance

- `relay --help`/argument-parse-error paths are byte-identical to before (no `init_tracing` call
  on those paths; `config::parse`'s own `tests/help_matches_docs.rs` stays green untouched).
- A real run with `RELAY_LOG=debug` (and `TXTODO_LOG` unset) produces JSON lines at debug level
  under `<data_dir>/logs/relay.log.<date>` and pretty lines on stderr.
- A real run with `TXTODO_LOG=debug` (and `RELAY_LOG` unset) does the same — proving the fallback.
- A real run with **both** set to different levels proves `RELAY_LOG` wins.
- `relay/tests/no_txtodo_deps.rs` still passes unmodified — the invariant this task discovered and
  deliberately worked around, not through.
- `cargo fmt -p relay -- --check`, `cargo clippy -p relay --all-targets -- -D warnings`,
  `cargo test -p relay` all green.

## As built (2026-09-16, agent)

Built per the design above. One real, load-bearing discovery changed the plan from what the
briefing assumed (call `txtodo_telemetry::init` directly): see "Blocking discovery" above —
`relay/tests/no_txtodo_deps.rs` forbids any `txtodo-*` dependency, by design (design §4.6), so
the JSON+pretty *shape* was reimplemented locally in `relay/src/main.rs` using
`tracing-appender`/`tracing-subscriber` directly instead.

- `relay/src/main.rs`: `init_tracing(data_dir) -> Option<LogGuard>` (JSON daily-rotated file under
  `<data_dir>/logs/relay.log.YYYY-MM-DD`, 7 kept via `prune_logs`, plus pretty stderr, one shared
  `EnvFilter`, `.try_init()` — never panics). `build_filter`/`resolve_log_filter` implement the
  `RELAY_LOG`-then-`TXTODO_LOG`-then-`"info"` precedence, the latter pure and injectable
  (mirrors `config.rs`'s own `Source`/`no_env` test pattern already in this crate). `main()` no
  longer calls `init_tracing()` before `config::parse` — moved into `run(config)`, once
  `data_dir` is known (mirrors `txtodo-daemon`'s `prepare_and_announce`, called from `run(args)`
  after `state_dir` resolves, not from `main()` before `Args` parsing). `--help`/parse-error paths
  are untouched (they never call `init_tracing`).
- `relay/Cargo.toml`: added `tracing-appender = "0.2"` (workspace-pinned version, not a `txtodo-*`
  crate) and `"json"` to `tracing-subscriber`'s features.
- `docs/relay.md`: new "Logging" section — `RELAY_LOG`/`TXTODO_LOG` precedence, file location,
  why this binary reimplements rather than depends on `txtodo_telemetry`.
- `deploy/systemd/relay.service`: one comment line noting `RELAY_LOG`/`TXTODO_LOG` now also
  control the JSON file under `$RELAY_DATA_DIR/logs`, alongside the pre-existing journal capture
  of stderr — no functional change to the unit itself (still no redirect needed).

### Verification

- `cargo fmt -p relay -- --check`: clean.
- `cargo clippy -p relay --all-targets -- -D warnings`: clean.
- `cargo test -p relay`: 13 lib unit tests + 4 new `main.rs` unit tests (`relay_log_wins_when_both_set`,
  `txtodo_log_is_the_fallback`, `info_when_neither_is_set`, `prune_logs_keeps_the_newest_seven_by_name`)
  + `docs_mention_optional`, `help_matches_docs`, `http_smoke` (2), **`no_txtodo_deps`** — all green.
  `no_txtodo_deps.rs` in particular is the proof the boundary this task discovered is still intact.
- Real manual run: `RELAY_LOG=debug RELAY_DATA_DIR=<tmp> relay --listen 127.0.0.1:18787`, killed
  after ~1.5s. Confirmed both sinks populated: a pretty line on stderr
  (`... INFO relay: relay listening addr=127.0.0.1:18787 data_dir=...`) and the identical event as
  one JSON line in `<tmp>/logs/relay.log.2026-09-16`
  (`{"timestamp":...,"level":"INFO","fields":{"message":"relay listening",...},"target":"relay"}`).
  `RELAY_LOG`-vs-`TXTODO_LOG` precedence itself is proven by the three new unit tests above (pure,
  no real env mutation needed) rather than by three separate manual runs.

### Deliberately out of scope / left as documented, not silently dropped

- No `service` field stamped onto relay's JSON lines (unlike every `txtodo_telemetry`-backed
  binary) — see "Edge cases" above for why.
- `crates/txtodo-telemetry` itself: untouched. No new public API was added there; the alias
  precedence lives entirely in `relay/src/main.rs`.
- `.claude/budgets.json`: untouched — `relay` has no `slices` entry, confirmed before starting.
- Any other `+m11 @observability` backlog line, or any file outside `relay/`, `docs/relay.md`,
  `deploy/systemd/relay.service`, `tasks/logging-relay-converge/`, and the one root todo.txt line.
