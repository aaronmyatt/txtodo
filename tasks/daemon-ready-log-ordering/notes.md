# daemon-ready-log-ordering

## Reported

Auditing why daemon startup feels flaky across clients (`txtodo`, `txtodo-tui`, `txtodo-mcp`,
`apps/desktop`) turned up a real ordering bug in the daemon's own startup sequence, separate from
`ref:daemon-always-available`'s spawn/autostart work.

## Current state

- `crates/txtodo-daemon/src/main.rs:341-395` startup order: registry → identity →
  relay/file-carrier → catalog → socket path → `prepare_and_announce` → `serve_global`.
- `log_ready` (`main.rs:314-317`) fires a `tracing::info!("daemon_ready")` **before** the actual
  `UnixListener::bind` call, which happens later inside `serve::serve_with`
  (`crates/txtodo-daemon/src/serve.rs:39-49`).
- No current consumer greps the `daemon_ready` log line — every client (`ensure_daemon` in
  `crates/txtodo-daemon-launch/src/spawn.rs`, `Daemon::wait_until_ready` in
  `crates/txtodo-tui/src/daemon.rs`) probes the socket directly with a raw connect, not the log —
  so this isn't causing today's flakiness.
- It's a landmine for the next thing that trusts the log line (health-check tooling, a future
  systemd `sd_notify`-style readiness signal, a test harness that greps daemon stdout instead of
  polling the socket).

## Design

Small, mechanical fix:

- Move the `log_ready` call (or the equivalent tracing event) to after `UnixListener::bind`
  succeeds inside `serve_with`, not before it in `main.rs`.
- If `main.rs` needs a pre-bind log for its own diagnostics, give it a distinctly-named event
  (e.g. `daemon_starting`) so the two are never confused.

## Out of scope

- The spawn/autostart mechanism itself (`ref:daemon-always-available`) — this only fixes the
  daemon's internal readiness signal, not who calls it or when.
- Any change to how clients determine readiness today (raw socket connect) — that stays as is.

## Acceptance

- `daemon_ready` (or its renamed equivalent) only appears in daemon logs after the Unix socket is
  actually accepting connections — verified by a test that races a probe against the log line.

## As built (2026-09-18)

- `crates/txtodo-daemon/src/serve.rs::serve_with` now emits `daemon_ready` itself, via a new
  `log_socket_bound` helper called immediately after `UnixListener::bind` succeeds — the true
  readiness signal, structurally guaranteed to be post-bind since it's textually after the `?`.
- `crates/txtodo-daemon/src/main.rs`'s old pre-bind event was renamed `daemon_starting`
  (`log_ready` → `log_starting`), so it can't be mistaken for readiness.
- Moving the event into `serve_with` pushed `serve.rs` over its cognitive-complexity budget, so
  the new log line lives in its own `log_socket_bound` fn (same split-for-budget pattern as
  `main.rs`'s pre-existing `log_ready`/`log_stopped`).
- Renaming also pushed `main.rs` over its 400-line file budget; `log_starting`/`log_stopped`/
  `start_boot_span` were extracted into a new `crates/txtodo-daemon/src/boot_log.rs` module (same
  pattern as `progress.rs`/`notes_registry.rs` being split out of their siblings).
- New regression test: `crates/txtodo-daemon/tests/ready_log_ordering.rs` spawns a real `txtodod`
  via the shared `support::Daemon` harness (whose own spawn already blocks on a retried real
  socket connect) and asserts `daemon_starting` precedes `daemon_ready` in the daemon's own JSON
  log. `cargo test -p txtodo-daemon --test ready_log_ordering` and
  `cargo clippy -p txtodo-daemon --all-targets` both green.
- Committed as `fe576ac`. `daemon-paths-shared-crate` (the next sibling task from the same
  daemon-consistency audit) is done too; `tui-global-socket-migration` and
  `desktop-autostart-env-respect` remain open.
