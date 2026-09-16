# txtodo-cli: the cli.command{name,mode} span + diagnostic eprintln! conversion (root todo.txt line 36)

## Goal

Root todo 36 asks for a `cli.command{name,mode}` span wrapping both dispatch functions
(`main.rs::dispatch_daemon`/`dispatch`), plus events at the daemon-vs-direct mode decision
(`client.rs::select`) and the mutation-expressible-vs-whole-file-write branch
(`daemon_mode.rs::plan_mutations`) — all three currently invisible. It also asks to convert the
three `eprintln!`s at `commands/edit.rs:66/107/146` (diagnostics wearing output's clothes) to
tracing events, and to wire `txtodo_telemetry::init` with a CLI-specific sink matrix: pretty
stderr at `warn`+ by default (quiet ordinary runs), JSON file only when `$TXTODO_LOG` is set.

## The println!-vs-eprintln!-vs-logging distinction (the actual point of this task)

- **108 `println!` calls, untouched.** These are the CLI's real output: todo lists, `TODO: N
  added.`-style confirmations, `env`'s resolved paths. `main.rs:3`'s
  `#![allow(clippy::print_stdout, clippy::print_stderr)]` exists precisely so these are never
  flagged. Not one of them was touched; verified by an exact `println!(` count before/after (see
  As built).
- **3 `eprintln!` calls, converted.** `commands/edit.rs:66` (`run_do`, already-marked-done),
  `:107` (`run_pri`, already-prioritized), `:146` (`run_depri`, not-prioritized) look like
  todo.sh-parity output (todo.sh's own `todo.sh` script `echo >&2`s the same text — see
  `tests/vendor/todo.sh:1261/1292/1462`) but are diagnostics: they report *why an item was
  skipped*, not the command's actual result. Confirmed safe to convert: `tests/todosh_parity.rs`
  compares only exit status and file contents, never stderr text (read in full before touching
  anything). Each becomes a dedicated `tracing::warn!` call (`cli.already_done`,
  `cli.already_prioritized`, `cli.not_prioritized`), logging only the `ITEM#` line-number string
  and, for priority, the single priority letter — never task text.
- **1 remaining `eprintln!`**, `main.rs`'s top-level `Err(e) => eprintln!("txtodo: {e}")` — the
  process's final fatal-error report when nothing else applies (e.g. a config parse failure
  before `Ctx` even exists, so no span/subscriber context to log into yet). Left alone: it isn't
  one of the three named lines, and it runs before/outside any tracing scope by construction.

## Design

### `cli.command{name,mode}` span

`dispatch` and `dispatch_daemon` are each split into a thin `#[tracing::instrument(name =
"cli.command", skip_all, fields(name = command_name(command), mode = "..."))]` wrapper plus an
`_inner` function carrying the original (already-branchy) match — the same wrapper/`_inner` split
`tasks/logging-mcp-call-span` used, for the same reason: `#[instrument]` itself costs
cognitive-complexity points, and both matches are already near the budget. `command_name(command:
&Command) -> &'static str` is a new, exhaustive (38-arm) match from `Command` variant to its
literal name — never `Debug`/`Display` on `Command` itself, which would recursively print every
field including free-text ones (`add "buy milk"`, `replace 3 "new text"`, etc.).

Both functions get their own span rather than one shared span threaded through: `dispatch_daemon`
sometimes calls `dispatch` directly (`Command::Env`) or indirectly through
`daemon_mode::run_via_daemon`'s scratch-copy closure (the todo.sh-command fallback arm) — so a
`mode = "daemon"` invocation can nest a `mode = "direct"` span inside it. That's intentional, not
a bug: the inner span is literally true (that sub-step really did run through the direct-mode
scratch-copy engine) and matches the task brief's literal ask for a span at *both* named
locations.

### `client.rs::select` event

`select` already had four early-return branches (`no_daemon`, per-dir socket, global socket,
neither). Rather than an inline `tracing::debug!` in each branch (the macro's own expansion adds
complexity on top of the branch it's in — the same trap noted in the task brief), each branch
calls a one-line `log_mode_selected(mode: &'static str, reason: &'static str)` — one macro call,
isolated in its own function, exactly the split `txtodo-telemetry`'s own tests
(`emit_info_event`/`emit_warn_event`) use for the identical reason. `mode`/`reason` are fixed
`&'static str` labels, never a path or other user text.

### `daemon_mode.rs::plan_mutations` event

`plan_mutations` (renamed `plan_mutations_inner`) keeps its exact original body; a new outer
`plan_mutations` wrapper calls it, then calls `log_mutation_plan(result.as_ref())` once. That
helper computes `(expressible: bool, mutation_count: usize)` via a plain match (no macro calls
inside its arms) and fires exactly one `tracing::debug!` afterward — again isolating the macro
expansion from any branch, this time from *two* call sites' worth of branching (the wrapper's own
match is trivial; the interesting branching stayed inside `_inner`, untouched).

### Sink matrix

`txtodo_telemetry::init(service, logs_dir)` (read in full before assuming new knobs were needed)
unconditionally builds *both* the JSON-rolling-file layer and the pretty-stderr layer, filtered by
one shared `EnvFilter` defaulting to `info` — exactly what `txtodo-mcp`/`txtodod` want, but not
what this task asks for the CLI: most invocations are one-shot (`txtodo add "x"`) and short-lived,
so an `info`-level default would print routine internals to stderr on every call, and creating a
rolling JSON file directory on every invocation nobody asked to log would be silent disk noise.
`init` exposes no separate "conditional file layer" or "different default level" knob — confirmed
by reading `lib.rs` in full — so the CLI builds its own two-path `init_telemetry`:

- `$TXTODO_LOG` unset (the common case): a lightweight `tracing_subscriber::fmt()` builder,
  stderr only, `LevelFilter::WARN`. No file, no `.txtodo/logs` directory created.
- `$TXTODO_LOG` set: delegates entirely to `txtodo_telemetry::init("txtodo", <dir>/.txtodo/logs)`
  — the shared JSON+stderr pair, filtered by whatever the variable says, same shape every other
  binary uses, logs alongside `txtodod`'s/`txtodo-mcp`'s own files in one directory.

`init_telemetry` reads `$TXTODO_LOG` via `std::env::var_os` directly rather than through
`config::Env` (`config.rs`'s own module doc: "the process environment ... injected, never read
below `main`"). This is a deliberate, narrow exception: telemetry bootstrap is infrastructure, not
CLI business logic, and every other txtodo binary's own `EnvFilter::try_from_env` reads the same
variable the same direct way, just hidden one layer deeper inside `txtodo_telemetry::init`. Adding
a public accessor to `config::Env` for this one variable was out of scope (task brief: touch only
`main.rs`, `client.rs`, `daemon_mode.rs`, `commands/edit.rs`).

`init_telemetry` is called once in `run()`, right after `Ctx` is built (needs `ctx.paths.dir` for
`.txtodo/logs`), before the `Doctor`/`Daemon`/`Mcp` early-return match — so every code path this
invocation can take is covered, not just the two dispatch functions.

## Placement

- `crates/txtodo-cli/Cargo.toml`: `txtodo-telemetry` (prod dep), `tracing = "0.1"` (prod, matches
  `txtodo-mcp`'s literal-version style rather than `.workspace = true`), `tracing-subscriber =
  { workspace = true }` (prod — unlike `txtodo-mcp`, this crate's own production code builds a
  subscriber directly for the default-quiet path).
- `.claude/budgets.json`: `slices.allowedDeps."txtodo-cli"` gains `"txtodo-telemetry"` (one line).
- `crates/txtodo-cli/src/main.rs`: `init_telemetry`, `command_name`, `dispatch`/`dispatch_daemon`
  wrapper+`_inner` split.
- `crates/txtodo-cli/src/client.rs`: `log_mode_selected`, called from each of `select`'s branches.
- `crates/txtodo-cli/src/daemon_mode.rs`: `log_mutation_plan`, `plan_mutations` wrapper,
  `plan_mutations_inner` (renamed original body).
- `crates/txtodo-cli/src/commands/edit.rs`: `log_already_done`/`log_already_prioritized`/
  `log_not_prioritized`, replacing the three `eprintln!`s.

## Edge cases

- `Command::Env` inside `dispatch_daemon_inner` calls `dispatch(ctx, command)` directly (not
  `dispatch_inner`) — so it still opens its own nested `mode = "direct"` span, consistent with
  every other path through `dispatch`.
- `plan_mutations`'s existing `#[cfg(test)]` unit tests call `plan_mutations` (the public wrapper)
  unchanged — they still exercise the exact same logic, just through one extra frame.
- `run_pri`'s diagnostic fires with the *new* requested priority letter (`new: char`), not the old
  one — matches the original `eprintln!("... already prioritized ({new}).")` text's own subject.
- The CLI's default-mode stderr layer does not carry the shared `service=` stamp
  `txtodo_telemetry::init`'s layers do (that stamping lives in `txtodo-telemetry`'s private
  `stamp` module, not reachable from here) — acceptable: the default path is a quiet, one-shot,
  human-facing fallback, not a multi-process log destined to be concatenated with `txtodod`'s.

## Acceptance

- `cli.command` span present at both `dispatch`/`dispatch_daemon`, fields exactly `name`/`mode`.
- Events fire at `select` (mode decision) and `plan_mutations` (mutation-expressible decision).
- The three `commands/edit.rs` `eprintln!`s become tracing events; no task/line text ever recorded
  on any new span or event — labels, ids, counts, single characters only.
- All 108 `println!` calls byte-for-byte untouched.
- Default run (no `$TXTODO_LOG`): no `.txtodo/logs` directory created; `warn`+ still visible on
  stderr. `$TXTODO_LOG` set: JSON file + pretty stderr, same shape as every other binary.
- `cargo fmt -p txtodo-cli -- --check`, `cargo clippy -p txtodo-cli --all-targets -- -D warnings`,
  `cargo test -p txtodo-cli` green, `tests/todosh_parity.rs` and `tests/daemon_mode.rs` specifically
  passing; no `#[allow]`/`#[expect]` anywhere new.

## As built (2026-09-16, agent)

Built exactly to the design above; no structural deviations.

### Verification

- `cargo fmt -p txtodo-cli -- --check`: clean.
- `cargo clippy -p txtodo-cli --all-targets -- -D warnings`: clean — no `cognitive_complexity` hit
  on `dispatch`/`dispatch_daemon` (thin wrappers), `dispatch_inner`/`dispatch_daemon_inner`
  (unchanged bodies), `select`, or `plan_mutations`/`plan_mutations_inner`; no `#[allow]`/
  `#[expect]` anywhere new.
- `cargo test -p txtodo-cli`: 45 unit tests (`--bin txtodo`) + every integration test file green,
  including `tests/todosh_parity.rs::every_scenario_leaves_byte_identical_files` (still compares
  only exit status and file bytes against real todo.sh — never stderr text, confirmed by reading
  the harness before converting anything) and all 10 tests in `tests/daemon_mode.rs`.
- `.claude/scripts/check-boundaries.sh`, `.claude/scripts/check-file-length.sh`: clean.
- **`println!` count, exact macro-invocation match (`println!(`, word-boundary, excluding
  `eprintln!(`) via `git show HEAD:<file>` for every file under `crates/txtodo-cli/src` vs the
  working tree: 108 before, 108 after — byte-identical, zero touched.** `eprintln!(` count: 4
  before (1 in `main.rs`, 3 in `commands/edit.rs`) → 1 after (only `main.rs`'s top-level fatal-error
  report remains). `git diff -- crates/txtodo-cli/src | grep -E '^[+-].*println!'` shows only the
  three removed `eprintln!` lines — no `println!` line was added, removed, or reformatted.
- Manual sink-matrix check (fresh scratch workspace, `--no-daemon`):
  - Two `do 1` runs with no `$TXTODO_LOG` set: second run's stderr shows the pretty
    `cli.already_done` warn line; `ls` on the workspace afterward shows **no** `.txtodo` directory
    at all — confirms the JSON file layer truly never opens without the env var.
  - Same command with `TXTODO_LOG=debug`: stderr shows `cli.mode_selected` (DEBUG) and
    `cli.already_done` (WARN, nested inside `cli.command{name="do", mode="direct"}`); `.txtodo/
    logs/txtodo.log.<date>` exists with matching JSON lines, each carrying `"service":"txtodo"`.
- `cargo check --workspace --all-targets` / the repo's `feedback.lint` (`cargo clippy --workspace
  --all-targets -- -D warnings`) still trips on `crates/txtodo-store/src/projections.rs::
  put_projection`/`put_snapshot` (cognitive complexity 11/10) — reconfirmed pre-existing and
  unrelated exactly as `tasks/logging-mcp-call-span/notes.md` and `tasks/logging-model/notes.md`
  already documented: `git stash && touch crates/txtodo-store/src/projections.rs && cargo clippy
  -p txtodo-store --all-targets -- -D warnings` passes clean on unmodified `main`. Left unfixed —
  not this task's crate. `cargo clippy -p txtodo-cli --all-targets -- -D warnings` (the task's
  actual acceptance gate) is unaffected and clean.

### Deliberately out of scope

- Any other `+m11 @observability` backlog line, or any file outside `crates/txtodo-cli/`
  (`main.rs`, `client.rs`, `daemon_mode.rs`, `commands/edit.rs`, `Cargo.toml`),
  `.claude/budgets.json` (one line), `tasks/logging-cli/`, and this one root todo.txt line.
- Adding a public `TXTODO_LOG` accessor to `config::Env` — would have kept the "env only read via
  `Env`" convention intact everywhere, but `config.rs` isn't one of the files this task named;
  see Design's "Sink matrix" section for the direct-`std::env::var_os` alternative taken instead.
- The pre-existing `txtodo-store` workspace-clippy cognitive-complexity failure (see Verification).
