# apps/desktop/src-tauri: subscriber, ipc{command} spans, ui_log, ensure_daemon (root todo.txt line 36)

## Goal

Root todo line (`ref:logging-desktop`) asks for: install the tracing subscriber in the Tauri
`.setup()` hook, `#[instrument]` an `ipc{command}` span on every `#[tauri::command]` fn, add a
`ui_log` command the frontend can forward into, instrument `daemon/spawn.rs::ensure_daemon`, and
fix `lib.rs`'s `let _ = commands::connect_and_store(...).await` silently discarding the startup
daemon-connect result.

## Which `txtodo_telemetry` entry point, and why

`txtodo_telemetry::init("desktop", &logs_dir)` (both JSON file + pretty stderr), **not**
`init_file_only`. `init_file_only` exists for exactly one reason (`crates/txtodo-telemetry/src/
lib.rs`'s own doc, `tasks/logging-tui/notes.md`): a process that owns a raw-mode/alternate-screen
terminal for its whole lifetime, where a stray stderr byte visibly corrupts the render. Checked
against how this app is actually launched (`apps/desktop/package.json`'s `tauri` script,
`apps/desktop/src-tauri/tauri.conf.json`'s `beforeDevCommand`/`build`): it's a normal windowed GUI
app — `tauri dev` runs it from an ordinary terminal (stderr just prints, no raw mode), and a built
`.app`/`.exe` bundle has no controlling terminal at all (stderr goes to the void, same as any other
GUI app double-clicked from a bundle) — no raw-mode/alternate-screen concern exists here at all, so
the plain `init` is both sufficient and strictly more useful (stderr is free diagnostic output for
the `tauri dev` case) with no corruption risk to avoid.

## Log directory

`config::global_state_dir().join("logs")`, not `<workspace>/.txtodo/logs`. Since ADR 0025
(`daemon-global-socket`), the desktop app dials one **device-global** `txtodod` that serves every
registered workspace, not a single workspace's own daemon — `config::global_state_dir()` is
already this crate's own copy of the same device-global state directory `ensure_daemon`'s spawn
lock uses (`daemon/spawn.rs`'s `SpawnGuard`). The global daemon itself writes `txtodod.log.*` to
`state_dir/logs` in true-global mode (`crates/txtodo-daemon/src/main.rs::prepare_and_announce`'s
own doc) — `desktop.log.*` lands in that same directory, so a human tailing one directory sees both
processes' lines, sortable by timestamp, the same "concatenate and still tell which process wrote
which line" property `txtodo-telemetry`'s own module doc describes. Tying the log directory to
`current_workspace` instead would be wrong twice over: that value is mutable at runtime
(`switch_workspace`) and no longer determines which daemon this process talks to.

## Macro-ordering finding: `#[tauri::command]` + `#[tracing::instrument]` (verified, not assumed)

Read `tauri-macros` 2.6.3's actual source
(`~/.cargo/registry/src/.../tauri-macros-2.6.3/src/command/wrapper.rs`, `pub fn wrapper`) rather
than assuming this matches the `rmcp`/`#[tool]` finding from `tasks/logging-mcp-call-span`.
`rmcp-macros`' `#[tool]` **rewrites** an `async fn` into a sync fn returning a boxed future — that
forces `#[tracing::instrument]` above it (closer to the original `async fn`), or instrument spans a
sync fn that merely returns an unstarted future and is never actually entered.

`tauri-macros`' `wrapper()` does **not** do this. It parses the annotated item as `syn::ItemFn` and
re-emits it completely unchanged into its output (`wrapper.rs:311-312`:
`quote!(#maybe_allow_unused #function ...)`) — no body rewrite, no boxing, no signature change. It
only emits an additional `macro_rules!` beside the untouched function, used later by
`tauri::generate_handler!` to register it. Whichever attribute macro applies first, the other still
sees (and, for `#[tracing::instrument]`, wraps) the real, unmodified `async fn` — so **either
attribute order compiles and spans correctly for a Tauri command**, unlike the rmcp case.

Confirmed empirically, not just by reading source: `commands.rs`'s `ui_log_emits_a_named_span_and_
the_forwarded_message` test calls `ui_log` (which carries `#[tracing::instrument(name =
"ipc.ui_log", ...)]` listed *above* `#[tauri::command]`) exactly the way `generate_handler!` would,
and asserts the `ipc.ui_log` span and its event both land in a real captured subscriber. The
reverse order (`#[tauri::command]` above `#[tracing::instrument]`) was also compiled by hand against
the same function during this task to confirm it type-checks identically — kept out of the final
diff since it adds no behavior, only proves the two orders are equivalent here.

Decision: kept `#[tracing::instrument]` **above** `#[tauri::command]` everywhere in this crate
anyway, purely for one consistent house style with `txtodo-mcp/src/schema.rs`'s `mcp.call`
convention — not because tauri requires it the way rmcp does. Documented inline at the top of
`commands.rs` so the next person who touches this crate doesn't have to re-derive it.

## `#[instrument]` on every `#[tauri::command]` fn

25 functions actually carry `#[tauri::command]` today (the root todo.txt line says 24 — a
one-off undercount at the time it was filed; every one found by `grep -rn '#\[tauri::command\]'
apps/desktop/src-tauri/src/` is covered, plus the new `ui_log` makes 26):

- `commands.rs` (11): `daemon_status`, `workspace_root`, `set_main_popover_dirty`, `retry_connect`,
  `list_files`, `get_file`, `watch`, `apply`, `history`, `resolve`, `list_conflicts` — plus the new
  `ui_log` (12 total in this file after this task).
- `commands_activity.rs` (1): `op_log`.
- `commands_notes.rs` (2): `get_notes`, `edit_notes`.
- `commands_pairing.rs` (3): `pair_offer`, `pair_accept`, `pair_confirm_sas`.
- `commands_tokens.rs` (3): `token_create`, `token_list`, `token_revoke`.
- `commands_universal.rs` (1): `universal_tasks`.
- `commands_workspace.rs` (4): `list_workspaces`, `add_workspace`, `remove_workspace`,
  `switch_workspace`.

Every one follows this crate's now-established `+m11 @observability` house style (same as
`crates/txtodo-tui`/`crates/txtodo-mcp`/`crates/txtodo-daemon`): the `#[tauri::command]` fn becomes
a thin wrapper — `#[tracing::instrument(name = "ipc.<name>", skip_all)]` — delegating to a
same-named `_inner` with the original, unmodified body. `#[instrument]`'s own expansion costs
`clippy::cognitive_complexity` points; splitting keeps the wrapper trivial regardless of how close
to budget the original body already was (`remove_workspace`, `universal_tasks` both have real
branches). `skip_all` on every single one, no exceptions — several of these commands carry real
task/note line text as an argument (`apply`'s `mutations`, `edit_notes`'s `new_text`, `resolve`'s
`task`), so this is not a style preference, it's the whole point (CLAUDE.md's logging rule, and
this task brief's own "highest-stakes crate so far for accidental leakage" framing).

Span names: `ipc.<function_name>` (e.g. `ipc.apply`, `ipc.list_conflicts`), matching the `tui.*`/
`mcp.call` naming convention already established elsewhere. No extra `fields()` beyond `skip_all`
on any of these 25 — unlike `mcp.call`'s `tool`/`principal` fields, there's no safe-to-log
identifier worth attaching to a Tauri command call that a span name doesn't already give (Tauri
itself already tags every invoke with the command name via its own `ipc::request::handler` debug
span when the `tracing` cargo feature is on, which this crate's `tauri` dependency does not enable
— these `ipc.*` spans are this crate's own, independent instrumentation).

## `ui_log`: the frontend's logging bridge (receiving end only)

```rust
#[tracing::instrument(name = "ipc.ui_log", skip_all, fields(level = %level))]
#[tauri::command]
pub fn ui_log(level: String, message: String, fields: Option<serde_json::Value>) -> Result<(), String>
```

`(level, message, fields?)`-shaped exactly as the task brief asked, matching this crate's existing
`Result<T, String>` error convention (every other command maps its error to `String` via
`.to_string()`; `ui_log` simply never fails, but keeps the same return shape other commands use
rather than introducing `()` as a one-off special case). `level` is one of `error`/`warn`/`info`/
`debug`/`trace` (anything else logs at `info`, never a hard error — a malformed level from the
frontend must not crash a log call). `fields = %level` on the span itself is safe to record (a
small closed-ish label, not content) — matches `tui.perform`'s `fields(action = action_kind(...))`
precedent for "a safe category label is fine, the payload itself never is."

`message`/`fields` are **not** skipped, unlike every other command in this crate — this is the one
deliberate exception, and it's exactly the feature, not an oversight: this command's entire purpose
is to carry whatever the frontend decided was safe to log into this process's own subscriber. What
belongs in `message`/`fields` is the caller's decision, made on the TypeScript side by the sibling
backlog item (`logging-frontend`) that calls this command — this task only builds the receiving
end, and does not touch any `.ts`/`.svelte` file per its own scope boundary.

Implementation: `ui_log` → `ui_log_inner` (thin wrapper split, matching every other command in this
file) → one small function per level (`log_ui_error`/`log_ui_warn`/`log_ui_info`/`log_ui_debug`/
`log_ui_trace`), each a single `tracing::<level>!` call — never a bare `tracing::*!` call inside
`ui_log_inner`'s own `match`, the same `log_mutation_ops`/`log_directory_event`/`log_ready_attempt`
pattern this whole `+m11` pass uses everywhere else, for the same cognitive-complexity reason.
`fields` (an `Option<serde_json::Value>`) is serialized once via `to_string()` (compact JSON,
`Display`, not `Debug`) before the `match`, so the emitted log line carries real JSON text in that
field instead of Rust debug syntax — proven by `ui_log_emits_a_named_span_and_the_forwarded_
message`'s round-trip parse.

Registered in `lib.rs`'s `invoke_handler!` as `commands::ui_log`.

## `ensure_daemon` instrumentation

`daemon/spawn.rs`'s `unix_impl::ensure_daemon` (the real implementation; `stub::ensure_daemon` on
non-unix is a one-line `Err(UnsupportedPlatform)` placeholder, not the function the task brief
names — same "don't instrument the placeholder" call `tasks/logging-tui/notes.md` made for its own
`#[cfg(not(unix))]` stub). Wrapper/`_inner` split, `#[tracing::instrument(name =
"desktop.ensure_daemon", skip_all)]` — `desktop.` prefix (not `ipc.`) since this isn't a
`#[tauri::command]`, it's an internal helper `connect_and_store` calls; naming it distinctly avoids
implying it's itself an IPC entry point. A `spawn_attempted` debug event fires (its own
`log_spawn_attempted()` function, not a bare call) only on the branch that actually spawns
`txtodod` — the common case (already-live daemon) logs nothing extra, keeping this quiet on the
hot path the same way `tasks/logging-tui/notes.md`'s `watch_dropped`/`ready_attempt` events stay
bounded-cardinality rather than firing on every call.

## `lib.rs:56` fix: no longer a silent discard

Before: `let _ = commands::connect_and_store(&handle, &state).await;` inside the `.setup()` hook's
background task — a failed boot connect left no trace anywhere, not even a log line, since nothing
else observes this particular background task (unlike a command's caller, which at least sees a
`Result` cross the IPC bridge). After: `if let Err(e) = ... { log_startup_connect_failed(&e); }` —
logs the error (`error = %e`, `DaemonError`'s own `Display`, never a debug dump of internal state)
via its own named function, isolated from the `if let` branch per this pass's usual style. Startup
behavior is otherwise **unchanged**: still non-fatal (the app still opens; `connect_and_store`'s own
`set_status` calls already drove `AppState::status` to `Dead` on failure, and the reconnect banner's
existing retry button — `commands::retry_connect` — is still the recovery path). This is
observability only, not a behavior change, matching the task brief's explicit instruction not to
guess at a different startup behavior.

## Placement

- `apps/desktop/src-tauri/Cargo.toml`: `txtodo-telemetry` (workspace dep), `tracing` (workspace
  dep), `tracing-subscriber` (workspace dep, dev-only — only the span-capture tests need it).
- `apps/desktop/src-tauri/src/lib.rs`: `txtodo_telemetry::init` call at the top of `run()`, the
  `let _log_guard` binding held for `run`'s whole body, `log_startup_connect_failed` helper, the
  `.setup()` fix, `commands::ui_log` added to `invoke_handler!`.
- `apps/desktop/src-tauri/src/commands.rs`, `commands_activity.rs`, `commands_notes.rs`,
  `commands_pairing.rs`, `commands_tokens.rs`, `commands_universal.rs`, `commands_workspace.rs`:
  every `#[tauri::command]` fn split into wrapper + `_inner`; `commands.rs` also gains `ui_log`/
  `ui_log_inner`/the five `log_ui_*` helpers and their test module.
- `apps/desktop/src-tauri/src/daemon/spawn.rs`: `ensure_daemon`/`ensure_daemon_inner` split,
  `log_spawn_attempted` helper.
- `.claude/budgets.json`: untouched — `slices.root` is `crates`, and `apps/desktop/src-tauri` is
  outside it, so this crate is not fence-guarded (no lease, no `allowedDeps` entry needed;
  confirmed by reading `.claude/hooks/fence.sh` and `budgets.json` directly before assuming).
- `tasks/logging-desktop/`: this file, `todo.txt`.
- Root `todo.txt`: this one line marked done.

## Edge cases

- `stub::ensure_daemon` (`#[cfg(not(unix))]`) is left untouched — not the function this task names
  (see above).
- `ui_log`'s `level` argument never crashes the command on an unrecognized value — falls back to
  `info`, since a broken log call must never be the thing that breaks the UI.
- `_log_guard`'s `.ok()`: an unwritable `global_state_dir()/logs` (e.g. read-only `$HOME` in some
  sandboxed CI) leaves the desktop app unlogged rather than failing to start — logging is
  diagnostic, the app is the product, same rule `txtodo-tui`/every other binary in this workspace
  already follows.
- `connect_and_store`/`ensure_connected` (the two shared helpers every command's `_inner` calls
  into) are deliberately **not** separately instrumented — the calling command's own `ipc.<name>`
  span already covers the whole call including these, and a nested span here would only add noise
  with no distinguishing information beyond "this ipc call happened to (re)connect."

## Acceptance

- `txtodo_telemetry::init("desktop", &config::global_state_dir().join("logs"))` installed in
  `run()`, guard held for the whole function.
- All 25 pre-existing `#[tauri::command]` fns (grep-confirmed) carry `#[tracing::instrument(name =
  "ipc.<name>", skip_all)]` via the wrapper/`_inner` split; `ui_log` (new, 26th) does too.
- `ui_log` exists, is registered in `invoke_handler!`, and its span + forwarded event are proven to
  reach a real subscriber by `ui_log_emits_a_named_span_and_the_forwarded_message`.
- `daemon/spawn.rs::ensure_daemon` (the unix implementation) carries `#[tracing::instrument(name =
  "desktop.ensure_daemon", skip_all)]` via the same wrapper/`_inner` split.
- `lib.rs:56`'s discarded `Result` is fixed: logged via `log_startup_connect_failed`, startup
  behavior otherwise unchanged.
- No `#[allow]`/`#[expect]` added anywhere.
- No command argument content (task text, note text, workspace paths beyond what already crossed
  the IPC boundary as a bare string argument) appears in any span or event this task added — see
  "As built" for the actual grep proof.
- `cargo fmt -p desktop -- --check`, `cargo clippy -p desktop --all-targets -- -D warnings`,
  `cargo test -p desktop` all green.

## As built (2026-09-16, agent)

Built exactly to the design above; no structural deviations. Four commits (this crate is outside
`slices.root` in `.claude/budgets.json`, so no fence lease was needed — verified by reading
`.claude/budgets.json`/`.claude/hooks/fence.sh` before assuming; commits were still split by
module for reviewability, per this task's own instructions):

1. `14d97db` — `txtodo-telemetry`/`tracing` deps, subscriber install in `lib.rs::run()`, the
   `lib.rs:56` fix, all 11 `commands.rs` commands + `ui_log` + its span-capture test.
2. `68cb876` — a leftover rustfmt fixup for `commands.rs`, plus `commands_activity.rs`/
   `commands_notes.rs`/`commands_pairing.rs`/`commands_tokens.rs` (9 commands).
3. `8c51ada` — `commands_universal.rs`/`commands_workspace.rs` (5 commands) and
   `daemon/spawn.rs::ensure_daemon`.

### Verification

- `cargo fmt -p desktop -- --check`: clean.
- `cargo clippy -p desktop --all-targets -- -D warnings`: clean — no `cognitive_complexity` hit on
  any of the 26 instrumented functions (every `#[tauri::command]`/`ensure_daemon` is a thin
  wrapper, every `_inner` keeps its original, unmodified body), no `#[allow]`/`#[expect]` added
  anywhere. A whole-workspace `cargo clippy --workspace` run (triggered automatically by this
  session's own PostToolUse hook after every edit) repeatedly flagged a pre-existing
  `txtodo-store::projections.rs` cognitive-complexity warning unrelated to this task — the exact
  known, flagged, transient noise this task's own brief warned about; confirmed not touched by any
  commit here (`git show --stat` on all three commits above touches only `apps/desktop/src-tauri/`,
  `Cargo.lock`, and `tasks/logging-desktop/`).
- `cargo test -p desktop`: lib tests (2, including the new `ui_log_emits_a_named_span_and_the_
  forwarded_message`) + all four integration test files (`daemon_spawn.rs` — 2, including a real
  spawn-and-connect of a freshly built `txtodod`; `new_rpcs.rs` — 7; `universal_view.rs` — 1;
  `workspace_registry.rs` — 2) all green, 14/14. `ensure_daemon`'s public signature is unchanged, so
  every pre-existing caller (including `daemon_spawn.rs`'s real-daemon tests) needed no changes.
- **Macro-ordering finding, proven empirically** (not just read from source): the `ui_log`
  span-capture test calls `ui_log` exactly as `tauri::generate_handler!` would (a direct async fn
  call with `#[tracing::instrument]` listed above `#[tauri::command]`), asserts the `ipc.ui_log`
  span and its event both land in a real `tracing_subscriber` JSON sink — proof the ordering
  chosen for all 26 instrumented functions in this crate actually works, not merely that it
  compiles. The reverse order was also hand-compiled against the same function during this task
  (kept out of the committed diff, since it adds no behavior beyond confirming the two orders are
  equivalent for `tauri::command` — unlike `rmcp`'s `#[tool]`, `tauri-macros`' `wrapper()` never
  rewrites the function body, confirmed by reading `tauri-macros` 2.6.3's actual source).
- **No command argument content leaks**: `grep -n 'tracing::instrument'` across every touched file
  shows `skip_all` on all 26 (see the grep output captured during this task — every `ipc.*`/
  `desktop.ensure_daemon` span). `grep -n 'tracing::\(info\|warn\|error\|debug\|trace\)!'` across
  the same files shows exactly three call sites carrying dynamic content: `ui_log`'s five
  `log_ui_*` helpers (the `message`/`fields` bridge — deliberate, see "ui_log" above, not an IPC
  argument leak since nothing else in this crate calls those helpers), `daemon/spawn.rs`'s
  `spawn_attempted` debug event (no arguments at all), and `lib.rs`'s `log_startup_connect_failed`
  (`error = %e`, a `DaemonError`'s `Display` — transport/IO/gRPC-status text, never task or note
  content). No `path`/`mutations`/`task`/`new_text`/`scopes`/`root`/`id` argument value from any of
  the 25 pre-existing commands appears in any span or event this task added.

### Deliberately out of scope

- Any `.ts`/`.svelte` file — `ui_log`'s frontend caller side is `logging-frontend`, a separate,
  later backlog item, per this task's own scope boundary.
- Any other `+m11 @observability` backlog line, or any file outside `apps/desktop/src-tauri/`,
  `tasks/logging-desktop/`, and this one root todo.txt line.
- `.claude/budgets.json` — untouched; this crate is outside `slices.root`, confirmed rather than
  assumed (see "Placement" above).
