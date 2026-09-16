# txtodo-tui: file-only log sink + event-loop instrumentation (root todo.txt line 36)

## Goal

Root todo line (`ref:logging-tui`) asks for: a file-only tracing sink for `txtodo-tui`
(`txtodo-tui.log.*` alongside the daemon's own logs, never stderr), plus `#[instrument]` on four
named sites: `app.rs:114` (the `select!` loop), `app.rs:176` (`perform` — the one place an `Action`
becomes an RPC), `app.rs:158` (`reconnect_watch`), and `daemon.rs:86`/`:106` (`connect` /
`wait_until_ready`).

## Why stderr is unsafe here (read from the actual code)

`app.rs::run` (line ~80) calls `ratatui::init()` before `run_loop` starts, and `ratatui::restore()`
only after it returns:

```rust
let mut terminal = ratatui::init();
let result = run_loop(&mut terminal, daemon, &mut state).await;
ratatui::restore();
```

`ratatui::init()` (confirmed by reading `ratatui-crossterm-0.1.2/src/lib.rs`, the backend this
crate's default features pull in) calls `crossterm::terminal::enable_raw_mode()` and executes
`EnterAlternateScreen` on stdout; `ratatui::restore()` reverses both. For the entire lifetime of
`run_loop` — which is the whole event loop, including every RPC in `perform`/`reconnect_watch` —
the process owns an alternate screen buffer in raw mode. Any stray byte written to stderr during
that window (a tracing pretty-layer default) is not redirected anywhere else the terminal is
managing; it lands on the same physical terminal underneath/interleaved with the alternate screen
and visibly corrupts the render (raw mode also disables the normal line-buffering/newline
translation a corrupted write would otherwise get cleaned up by). There is no window before
`ratatui::init()` where the sink could safely use stderr and then switch off it either: `main`
calls `daemon.wait_until_ready()` and `Daemon::connect` (the two `daemon.rs` instrument sites)
*before* `run` is called, i.e. before raw mode — but the log guard/subscriber must already be
installed by then to capture those, and once installed it stays installed for the raw-mode window
too. So the only sink that is safe for the whole process lifetime is one that never touches stderr
at all.

## Design

### File-only sink: `txtodo_telemetry::init_file_only`

Read `crates/txtodo-telemetry/src/lib.rs` in full first: `init(service, logs_dir)` unconditionally
builds *two* layers — a JSON rolling-file layer and a pretty stderr layer — composed onto one
`tracing_subscriber::registry()`, both sharing one `EnvFilter`. There is no existing parameter or
mode to build only one of the two; adding a second entry point is required, not a config tweak.

Added `pub fn init_file_only(service: &'static str, logs_dir: &Path) -> io::Result<LogGuard>` to
`txtodo-telemetry`: same `LogGuard`, same `EnvFilter`/`TXTODO_LOG` semantics, same
`<service>.log.YYYY-MM-DD` daily rotation + prune-to-7 behavior, same `service` field stamped on
every line — but registers only the JSON layer, never `tracing_subscriber::fmt::layer()`'s stderr
writer. The JSON-rolling-file setup (`create_dir_all`, `prune`, `tracing_appender::rolling::daily`,
`non_blocking`, the `stamp::json_writer`-wrapped `fmt::layer().json()`) is shared via a new private
`build_json_layer<S>(service, logs_dir) -> io::Result<(impl Layer<S> + Send + Sync, WorkerGuard)>`
generic over the subscriber type, called by both `init` and `init_file_only` — so the JSON layer is
built in exactly one place in the source, not copy-pasted. `init` composes
`registry().with(filter).with(json_layer).with(pretty_layer)`; `init_file_only` composes
`registry().with(filter).with(json_layer)` and stops there — structurally incapable of writing to
stderr, not just configured not to.

### Call site: `txtodo-tui`'s `async_main`

`app.rs::async_main` calls `txtodo_telemetry::init_file_only("txtodo-tui",
&workspace.join(".txtodo/logs"))` right after resolving `workspace` (the same directory
`daemon.rs::socket_path` uses, so `txtodo-tui.log.*` lands next to `txtodod.log.*` in one
`.txtodo/logs/` directory — "alongside the daemon's", per the task brief) and before `Daemon::
connect`, so both `daemon.rs` instrument sites are covered. The `LogGuard` is held in a local
binding for the rest of `async_main`'s scope (dropped, and its non-blocking writer flushed, only
after `run` returns) — the same "hold until the end of the function that owns the process's
human-output window" pattern `txtodo-cli`'s `main.rs::run` uses for its own `_log_guard`. Init
failure (e.g. an unwritable `.txtodo/logs`) is swallowed with `.ok()`, exactly like
`txtodo-daemon`/`txtodo-mcp`'s own fallback-tolerant style elsewhere in this workspace: a dead
logger must never stop the TUI from running, since logging is diagnostic, not functional.

### The four instrument sites

Following this repo's established `+m11 @observability` style (`crates/txtodo-daemon/src/
mutation.rs`, `watcher.rs`, `crates/txtodo-cli/src/main.rs`, etc.): every `#[instrument(skip_all)]`
function is a thin wrapper delegating to a same-named `_inner` (the attribute's own macro expansion
costs `clippy::cognitive_complexity` points, and several of these functions are already
branch-heavy), and no bare `tracing::debug!`/`warn!`/`error!` call sits inside a `match`/`if`
branch — each such call is isolated in its own one-line function, called after the branch, the same
`log_mutation_ops`/`log_directory_event` pattern `mutation.rs`/`watcher.rs` use.

- **`app.rs::run_loop`** (contains the `select!` at line 114) → `run_loop` becomes a thin
  `#[tracing::instrument(name = "tui.run_loop", skip_all)]` wrapper around `run_loop_inner`
  (unchanged body). A `watch_dropped` debug event fires, from its own `log_watch_dropped()`
  function, on the reconnect branch of the `select!` (never inside the `if let`/`else` itself).
- **`app.rs::perform`** → wrapper `#[tracing::instrument(name = "tui.perform", skip_all,
  fields(action = action_kind(&action)))]` around `perform_inner` (unchanged body).
  `action_kind(&Action) -> &'static str` maps `Quit`/`Apply`/`Resolve` to their label — never
  `Debug`/`Display` on `Action` itself (would print the `ApplyRequest`/`ResolveRequest` payload,
  including task line text).
- **`app.rs::reconnect_watch`** → wrapper `#[tracing::instrument(name = "tui.reconnect_watch",
  skip_all, fields(attempt = *reconnects + 1))]` around `reconnect_watch_inner` (unchanged body).
- **`daemon.rs::Daemon::connect`** (the `#[cfg(unix)]` arm only — the `#[cfg(not(unix))]` stub is
  an unreachable placeholder, not the line the task names) → wrapper
  `#[tracing::instrument(name = "tui.daemon_connect", skip_all)]` around `connect_inner`.
- **`daemon.rs::Daemon::wait_until_ready`** → wrapper `#[tracing::instrument(name =
  "tui.wait_until_ready", skip_all)]` around `wait_until_ready_inner`. A `ready_attempt` debug
  event (own `log_ready_attempt(attempt, ok)` function) fires once per probe, called after the
  `match`, not inside either arm.

No span or event anywhere carries task/line text, file bytes, or a socket path's full contents —
only ids (`attempt` counters), counts, and named event labels, exactly the "never log payload
bytes, task line text, or secrets" rule this whole `+m11` pass has followed elsewhere.

## Placement

- `crates/txtodo-telemetry/src/lib.rs`: `build_json_layer` (private, generic, shared), `init`
  refactored to call it, new `pub fn init_file_only`.
- `crates/txtodo-tui/Cargo.toml`: `txtodo-telemetry` (prod dep, path), `tracing = "0.1"` (prod,
  literal version — matches `txtodo-mcp`'s style, this crate's own production code now calls
  `tracing::instrument`/`tracing::debug!` directly).
- `.claude/budgets.json`: `slices.allowedDeps."txtodo-tui"` gains `"txtodo-telemetry"` (one line).
- `crates/txtodo-tui/src/app.rs`: `init_file_only` call in `async_main`; `run_loop`/`run_loop_inner`
  split, `perform`/`perform_inner` split + `action_kind`, `reconnect_watch`/`reconnect_watch_inner`
  split, small `log_*` helper functions.
- `crates/txtodo-tui/src/daemon.rs`: `connect`/`connect_inner` split (unix arm only),
  `wait_until_ready`/`wait_until_ready_inner` split, `log_ready_attempt` helper.
- `tasks/logging-tui/`: this file, `todo.txt`.
- Root `todo.txt`: mark this one line done.

## Edge cases

- `#[cfg(not(unix))]`'s `Daemon::connect` stub is untouched — it's a one-line
  `Err(UnsupportedPlatform)` placeholder for a platform this crate doesn't build a real transport
  for yet, not the `connect` the task brief names (that's the `#[cfg(unix)]` arm at line 86).
- `init_file_only`'s failure path (`.ok()`) means a broken `.txtodo/logs` directory silently leaves
  the TUI unlogged rather than crashing the terminal session — logging is diagnostic, the terminal
  session is the product.
- `LogGuard` is held for the whole of `async_main`, not just `run`, so `wait_until_ready`'s and
  `connect`'s own instrument spans (which run before `run`/`ratatui::init()`) are captured too.
- `run_loop`'s `watch_dropped` event and `wait_until_ready`'s `ready_attempt` event are the only two
  "inside a loop" events in this pass; both are named, bounded-cardinality labels (a `select!`
  iteration and a bounded retry loop respectively), not per-keystroke or per-frame — logging every
  `terminal.draw` call would be noise with no diagnostic value and was deliberately not added.

## Acceptance

- `init_file_only` exists in `txtodo-telemetry`, builds only the JSON layer (no stderr layer,
  confirmed by reading the function body and by a grep for `stderr`/`fmt::layer()` without `.json()`
  in it), reuses the shared JSON-layer construction rather than duplicating it.
- `txtodo-tui.log.YYYY-MM-DD` lands in `<workspace>/.txtodo/logs/`, alongside `txtodod.log.*`.
- All four named sites (`app.rs:114`/`:158`/`:176`, `daemon.rs:86`/`:106`) carry
  `#[tracing::instrument(skip_all)]` via the wrapper/`_inner` split, no `#[allow]`/`#[expect]`
  anywhere new.
- No stderr write is introduced anywhere in the code paths this task touches — verified by reading
  every changed file and grepping for `eprintln!`/`stderr` in the diff.
- `cargo fmt -p txtodo-tui -- --check`, `cargo clippy -p txtodo-tui --all-targets -- -D warnings`,
  `cargo test -p txtodo-tui` all green; same for `-p txtodo-telemetry`.

## As built (2026-09-16, agent)

Built exactly to the design above; no structural deviations. Two commits, not one: the fence's
"one crate leased per session" rule meant `txtodo-telemetry`'s `init_file_only` had to be
implemented and committed on its own before `txtodo-tui`'s lease could be taken in the same
session (the lease file is `<git-common-dir>/txtodo-leases/<crate>.lock`, released once the gate's
Stop hook sees a clean tree — since this session never actually hits that Stop boundary mid-task,
the lease was removed by hand once the `txtodo-telemetry` commit landed, the same recovery fence.sh
itself documents for an abandoned lease, applied here to a lease this same session had already
"finished and committed" per that hook's own release condition).

### Verification

- `cargo fmt -p txtodo-telemetry -- --check` / `-p txtodo-tui -- --check`: both clean.
- `cargo clippy -p txtodo-telemetry --all-targets -- -D warnings` / `-p txtodo-tui --all-targets --
  -D warnings`: both clean — no `cognitive_complexity` hit on any of the four instrumented sites
  (`run_loop`/`perform`/`reconnect_watch`/`connect`/`wait_until_ready` all thin wrappers,
  `run_loop_inner`/`perform_inner`/`reconnect_watch_inner`/`connect_inner`/`wait_until_ready_inner`
  unchanged-shape bodies), no `#[allow]`/`#[expect]` anywhere new.
- `cargo test -p txtodo-telemetry`: 6/6 green (unchanged — `init`'s own behavior wasn't touched,
  only refactored to share `build_json_layer`; `init_file_only` isn't unit-tested with a real
  `try_init()` call, matching this crate's own established pattern — `testing.rs`'s module doc
  explicitly built `capturing_dispatch` so tests never touch "the process-global subscriber another
  test in the same binary may already hold via `try_init()`"; the same reason `init` itself has no
  such test either).
- `cargo test -p txtodo-tui`: 43 unit tests + 7 integration tests (`tests/roundtrip.rs`,
  `tests/external_edit.rs`) green — unchanged in count and content, since `perform`/
  `reconnect_watch`'s public signatures and behavior are byte-identical, only wrapped.
- `.claude/scripts/check-boundaries.sh`, `.claude/scripts/check-file-length.sh`: clean.
- `cargo deny check`: `advisories ok, bans ok, licenses ok, sources ok` (pre-existing, unrelated
  `zbus`/`keyring` duplicate-version warnings only, nothing from `tracing`/`txtodo-telemetry`).
- **No stderr write introduced**: `grep -n 'eprintln!\|stderr' crates/txtodo-tui/src/app.rs
  crates/txtodo-tui/src/daemon.rs` shows only the five pre-existing `eprintln!` calls in `main`/
  `async_main` (already `#[allow(clippy::print_stderr)]`-marked, untouched by this task) — all five
  run either before `ratatui::init()` (the `Daemon::connect`/`wait_until_ready` failure branches)
  or after `ratatui::restore()` has already run (`run()`'s own `Err` arm, since `run` restores the
  terminal internally before returning). `daemon.rs` has zero `eprintln!`/`stderr` references at
  all. `crates/txtodo-telemetry/src/lib.rs`'s `init_file_only` body itself never references
  `std::io::stderr` or `tracing_subscriber::fmt::layer()` without `.json()` — only `build_json_layer`
  is composed onto its registry.
- Manual sink check (not run — would need a real `txtodod` and a real terminal; deferred to the
  human per CLAUDE.md §2.3 "expensive visual checks — ask first"). The structural proof above (the
  file-only registry composition, the grep, and the raw-mode-window reasoning in Design) is the
  verification this task actually asks for; a human running `txtodo-tui` against a real workspace
  with `TXTODO_LOG=debug` and confirming `.txtodo/logs/txtodo-tui.log.<date>` fills while the
  terminal renders cleanly is the natural follow-on eyes-on-glass check.

### Deliberately out of scope

- Any other `+m11 @observability` backlog line, or any file outside `crates/txtodo-tui/`,
  `crates/txtodo-telemetry/src/lib.rs` (the one additive, minimal entry point),
  `.claude/budgets.json` (one line), `tasks/logging-tui/`, and this one root todo.txt line.
- A real end-to-end "start `txtodo-tui`, watch the terminal not corrupt" run — see the manual
  sink-check note above.
