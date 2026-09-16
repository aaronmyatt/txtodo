# txtodo-daemon: converge on txtodo-telemetry, put the boot story in the JSON log (root todo.txt line 36)

## Goal

Finish what `logging-telemetry-crate` (root todo.txt 173) left blocked: swap `txtodo-daemon`'s own
`telemetry.rs` to a thin re-export of the new shared `txtodo_telemetry` crate, and make `txtodod`'s
own boot sequence show up in its structured logs instead of on bare stderr. Today two real lines —
"txtodod ready"/"txtodod stopped" — bypass the `tracing` subscriber entirely via `eprintln!`, so
they never land in the JSON file a human or another tool would grep. `workspace_catalog.rs:183`
also has one prose-sentence log event where every sibling site in that file uses a snake_case event
name (`workspace_route_registration_failed` at `workspace_catalog_open.rs:183`,
`lan_shared_session_started` in `lan_session_dispatch.rs`) — a small consistency fix riding along.

## Design

### `telemetry.rs`: thin re-export

`crates/txtodo-daemon/src/telemetry.rs` becomes `pub use txtodo_telemetry::{LOG_FILTER_ENV,
LOG_KEEP_FILES, LogGuard, prune};` plus `pub fn init(logs_dir) -> io::Result<LogGuard> {
txtodo_telemetry::init("txtodod", logs_dir) }` — exactly the shape
`tasks/logging-telemetry-crate/notes.md`'s "Placement / dependencies" section already designed.
`main.rs`'s existing call site (`txtodo_daemon::telemetry::init(&state_dir.join("logs"))`) needed no
signature change. The old `LOG_FILE_PREFIX` constant and the in-crate `prune()` test are dropped —
neither had any caller outside `telemetry.rs` itself (verified by grep), and the equivalent test
(`service_field_present_on_every_emitted_line`, `prune_keeps_the_newest_seven_by_name`) already
lives in `txtodo-telemetry`'s own `lib.rs`.

### `daemon.boot` span: entered for the boot sequence only, not the daemon's whole lifetime

`run()` computes `mode` (`"dir-bridge"` when `--dir` was given, else `"global"`) and opens
`tracing::info_span!("daemon.boot", version, mode, socket = Empty)` via a tiny `start_boot_span`
helper (kept separate purely so the macro's own expansion doesn't push `run`'s cognitive complexity
over budget — the same reason `prepare_and_announce` was already split out of `run` before this
task). The span is `.enter()`-ed immediately, held as a local guard (`_boot`) across every `.await`
in the boot sequence (registry open, identity build, relay bind, file-carrier open, catalog
construction, socket resolution), and **explicitly `drop()`-ped right after `prepare_and_announce`
returns** — before `serve::serve_global(...).await` starts the long-running accept loop. Two
deliberate choices here:

- **Held across `.await` at all.** `tracing::span::Entered` is `!Send`, so this only works because
  `run`'s future is driven directly by `rt.block_on(run(args))` in `main`, never `tokio::spawn`-ed —
  the exact same precondition `tasks/logging-telemetry-crate/notes.md`'s "Field name decision" →
  "service field" section already documented (and already relies on, for `LogGuard`).
- **Dropped before the serve loop, not left open for the daemon's whole life.** A span entered via
  `.enter()` (not `.instrument()`) pins the thread-local span stack for as long as the guard is
  alive *and this task's future is being polled* — leaving it entered across the whole multi-hour
  `serve_global` await would risk any *other* task polled on the same OS thread while this one is
  suspended (the tokio multi-thread runtime interleaves tasks per-thread) picking up `daemon.boot`
  as its parent span by accident. Scoping the guard to just the synchronous-ish boot sequence, which
  this codebase already treats as one unit (`prepare_and_announce`'s own pre-existing doc calls it
  "the ... sequence"), keeps the blast radius to exactly the window the task asked for.

`socket` starts as `tracing::field::Empty` (unknown at span-creation time — it depends on which of
`start_dir_bridge`/`start_global` runs) and is recorded once known, from inside `log_ready`
(`tracing::Span::current().record("socket", ...)`) rather than as a second statement in `run` —
folding it into the same already-small helper that emits the `daemon_ready` event keeps `run`'s own
complexity from creeping back over budget with two more statements.

### The two converted `eprintln!`s

- `main.rs:340`'s `"txtodod ready: socket {}, registry {}"` → `log_ready`, a
  `tracing::info!(socket = %.., registry = %.., "daemon_ready")` event nested inside `daemon.boot`
  (it runs from `prepare_and_announce`, called while `_boot` is held).
- `main.rs:394`'s `"txtodod stopped"` → `log_stopped`, `tracing::info!("daemon_stopped")`, fired
  *after* `_boot` was dropped and `serve_global` has returned — deliberately not nested inside
  `daemon.boot` (it isn't part of booting).

Both were split into their own one-line private functions (`log_ready`, `log_stopped`,
`start_boot_span`) for the same reason `prepare_and_announce` already exists: `cargo clippy`'s
`cognitive_complexity` lint counts a tracing field macro's own expansion against whichever function
it's written in, and `run`/`prepare_and_announce` were already close to the 10-point budget before
this task added more tracing calls to either.

### Sink matrix: no foreground/service detection needed

The task's own brief specifies "JSON file always, pretty stderr foreground only". Investigated
whether `txtodod` needs to detect foreground-vs-service at runtime (e.g. via `IsTerminal` on
stderr) — it does not: `txtodo_telemetry::init` already installs *both* layers unconditionally
(JSON rolling file + pretty stderr, `crates/txtodo-telemetry/src/lib.rs`), and the two deploy
manifests already handle the "foreground only" half for free by redirecting this binary's stderr
elsewhere the moment it's not foreground:

- `deploy/launchd/com.txtodo.txtodod.plist`: `StandardErrorPath` redirects stderr to
  `{{LOGDIR}}/launchd.err.log`, a file separate from the JSON log.
- `deploy/systemd/txtodod.service`: no redirect directive, so systemd's own default captures stderr
  into the user journal (`journalctl --user -u txtodod`), also separate from the JSON log.

So "pretty stderr, foreground only" is already true in practice, by deployment plumbing that
predates this task, not by code in this crate — added no `IsTerminal`/env-var detection, since doing
so would just be reimplementing what the OS-level redirect already gives for free, and would add a
third, harder-to-audit way the two sinks could end up diverging.

### `workspace_catalog.rs:183`: prose → snake_case event name

`tracing::warn!(root = %.., error = %.., "could not open registered workspace")` →
`"workspace_open_failed"`, matching the sibling convention in the same crate
(`workspace_route_registration_failed`, `lan_shared_session_started`). Left
`workspace_catalog.rs:73`'s own `"could not list the workspace registry"` untouched — the backlog
line names only `:183`, and `:73` is a different, unnamed line outside this task's stated scope.

### Test migration: `lan_session_security_tests.rs` / `security_m8_tests.rs`

`lan_session_security_tests.rs` drops its own `LogSink`/`capturing_dispatch`/`captured_text` (kept
`hex()`, daemon-test-specific, not part of the shared seam) and imports
`txtodo_telemetry::testing::{LogSink, capturing_dispatch}` instead; added
`pub(crate) const SERVICE: &str = "txtodod"` so both this file's own test and `security_m8_tests.rs`
stamp the same service name `capturing_dispatch` now requires. `security_m8_tests.rs` (found by the
previous session, not in the original `logging-telemetry-crate` plan) imports
`txtodo_telemetry::testing::{LogSink, capturing_dispatch}` directly rather than through
`lan_session_security_tests`'s re-export (the module doc pattern `security_m8_tests.rs`'s own header
already asked for: reuse via `pub(crate)` visibility, not duplication) and still uses
`lan_session_security_tests::{SERVICE, hex}` for the two pieces that stay there.

## Placement / dependencies

- `crates/txtodo-daemon/Cargo.toml`: one new line, `txtodo-telemetry.workspace = true`.
- `.claude/budgets.json`: `"txtodo-telemetry"` appended to `txtodo-daemon`'s own `allowedDeps` list
  only — the CLI/TUI/MCP entries `logging-telemetry-crate` originally planned are each that
  binary's own future task's job (items 182/183/181), not this one's.
- Touched: `crates/txtodo-daemon/src/telemetry.rs` (rewritten), `crates/txtodo-daemon/src/main.rs`
  (`daemon.boot` span, `log_ready`/`log_stopped`/`start_boot_span`), `crates/txtodo-daemon/src/
  workspace_catalog.rs` (one event name), `crates/txtodo-daemon/src/lan_session_security_tests.rs`
  and `crates/txtodo-daemon/src/security_m8_tests.rs` (import migration).
- Not touched: `crates/txtodo-telemetry/**` (read-only dependency), any other `+m11 @observability`
  backlog line/crate, root `Cargo.toml`/`Cargo.lock` (the workspace dependency entry already exists
  from `logging-telemetry-crate`; `txtodo-daemon`'s own manifest only needed one added line).

## Edge cases

- `main.rs` was already at the 400-line file-length budget before this task (`fileLines` in
  `.claude/budgets.json`); every doc comment touched by this change (including several *not*
  otherwise related to logging) was tightened to make room for the new span/event code without
  exceeding it — see the file's own diff for exactly which comments shrank. No comment lost its
  load-bearing fact, only its extra words.
- `boot_span.record("socket", ...)` needed a value that outlives the call: `socket.display()`
  borrows `socket`, so `.to_string()` first, then `.as_str()` on that owned `String` — a `&str`
  `Value` is only read synchronously by `record`, so the temporary's lifetime is sufficient.
- `log_ready`'s `tracing::Span::current()` resolves to `daemon.boot` correctly because it always
  runs synchronously inside `prepare_and_announce`, itself always called while `run`'s `_boot` guard
  is alive — if a future refactor ever calls `log_ready` from outside that window, the `record` call
  becomes a silent no-op (no span to attach the field to) rather than a panic; flagged here so that
  isn't a surprise later.
- `--dir` bridge vs. global mode: `mode` is computed from `args.dir.is_some()` via
  `Option::map_or` rather than an `if`/`else` block — not a style preference, `rustfmt` (stable,
  this repo's pinned `rustfmt.toml` has no `single_line_if_else_max_width`, a nightly-only option)
  always expands an `if`/`else`-as-value onto five lines regardless of width, which would have
  reopened the file-length problem this task already had to solve once.

## Acceptance

- `cargo fmt -p txtodo-daemon -- --check`: clean.
- `cargo clippy -p txtodo-daemon --all-targets -- -D warnings`: clean.
- `cargo test -p txtodo-daemon`: green except one pre-existing, unrelated flake — see "As built".
- Root todo.txt's `logging-daemon-boot` line and every subtask below marked done.

## As built (2026-09-16, agent)

Built exactly as designed above. `.txtodo/logs/txtodod.log.*` from a real test run (`pairing_relay`
integration test, captured mid-investigation below) shows the new events landing correctly:

```json
{"service":"txtodod","...","fields":{"message":"starting","dir":"Some(\"...\")","version":"0.0.0"},"target":"txtodod"}
{"service":"txtodod","...","fields":{"message":"daemon_ready","socket":"/.../txtodod.sock","registry":"/.../registry.db"},"target":"txtodod"}
```

`service` on both lines confirms `telemetry.rs`'s re-export is actually wired to
`txtodo_telemetry::init` end to end, not just compiling.

### One line-budget wrinkle not anticipated in the design

`main.rs` was sitting exactly at `fileLines: 400` before this task even started (`.claude/
budgets.json`). Every addition — the `daemon.boot` span, `start_boot_span`, `log_ready`,
`log_stopped` — pushed it well past 400 (peaked at 418 mid-edit) purely from new doc comments and
function boilerplate, *before* any of the feature logic itself was the problem. Paid it down by
tightening prose in doc comments across most of the file's existing functions (not just the ones
this task touched functionally) rather than deleting content — every fact that was there before is
still there, just fewer words per fact. Landed at exactly 400 lines.

### Cognitive-complexity wrinkle: tracing macros count against the caller

`cargo clippy`'s `cognitive_complexity` lint (budget 10) analyzes post-macro-expansion code, so each
`tracing::info!`/`info_span!` call's own internal `is_enabled` branching counts against whichever
function textually contains it — not against `tracing` itself. Adding a second `tracing::info!` to
`prepare_and_announce` (already carrying one) pushed it from passing to 16/10; adding the boot span
and a `daemon_stopped` event directly in `run` pushed it to 11/10, then 12/10 once the span's
`record()` call and the stopped-event macro were both inline. Splitting each tracing call into its
own tiny one-purpose function (`log_ready`, `log_stopped`, `start_boot_span`) brought both functions
back under budget — the same reason `prepare_and_announce` already existed as a split out of `run`
before this task touched either.

### Test result: one real, pre-existing, unrelated failure

`cargo test -p txtodo-daemon` (full crate: unit + every `tests/*.rs` integration binary) is green
except `tests/pairing_relay.rs::two_real_daemons_pair_over_relay_with_lan_disabled`, which failed
once in this pass's own run ("the group key never landed on the joiner within 110s"). This is not a
regression from this task's changes:

- The test's own module doc (`tests/pairing_relay.rs:24-32`, pre-existing, unmodified by this task)
  already documents this exact relay endpoint (a real public `n0.iroh.link` server) as "measured
  directly, repeatedly, as unusually unreliable" — one prior run needed ~24 failed connection
  attempts before the 25th succeeded.
  - The sibling test in the same file (`a_relay_dial_with_the_wrong_nonce_cannot_complete_a_pairing`)
    is already `#[ignore]`d for a related, separately-diagnosed real-relay-timing issue.
  - Recent root todo.txt/commit history independently confirms this area is a known, tracked gap:
    "chore(tasks): record real gap found testing — pairing over relay never dials for ongoing sync"
    and "test(cli): quarantine the pairing convergence assert, keep the ceremony running" (both
    immediately preceding commits, unrelated to this task).
- This task's diff touches none of `pairing_relay.rs`, `pairing_relay_dial.rs`, `pairing_grpc.rs`,
  or any relay/pairing code — only `telemetry.rs`, `main.rs`, `workspace_catalog.rs`, and the two
  security test files' imports.
- Every other integration test binary in the crate (`crash`, `debug_hooks`, `editor_saves`,
  `external_edits[_sidecar]`, `file_carrier_converge`, `global_socket`, `grpc`, `idle_rss`,
  `lan_discovery`, `lan_loopback_converge`, `lan_sync_bench`, `m5_acceptance`, `nested_ref_sync`,
  `notes_grpc`, `pairing_lan`, plus the crate's own unit tests including the migrated
  `lan_session_security_tests`/`security_m8_tests`) passed, including the other three tests in
  `pairing_relay.rs` itself.

Not re-run to try to get a green result by luck — a flaky, network-dependent, already-documented gap
in an unrelated subsystem, left exactly as found, flagged here rather than silently retried until it
passed.

## References

- `crates/txtodo-telemetry/src/lib.rs`, `crates/txtodo-telemetry/src/testing.rs` (the dependency).
- `tasks/logging-telemetry-crate/notes.md` (the design this task's `telemetry.rs`/test-migration
  pieces were originally planned as part of, before being redirected here mid-flight).
- `deploy/launchd/com.txtodo.txtodod.plist`, `deploy/systemd/txtodod.service` (the sink-matrix
  evidence).
- `crates/txtodo-daemon/tests/pairing_relay.rs` (the pre-existing flaky test, not touched).
