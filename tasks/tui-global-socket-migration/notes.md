# tui-global-socket-migration

## Reported

Auditing daemon-startup consistency across clients: `crates/txtodo-tui` is the only client left
dialing a different daemon than the rest of the fleet.

## Current state

- `ref:daemon-always-available` unified `txtodo`, `txtodo-mcp`, and `apps/desktop` onto the
  device-global `txtodod` socket, with shared autostart via `crates/txtodo-daemon-launch`.
- `crates/txtodo-tui/src/app.rs:82-92` (`async_main`) still constructs its `LaunchConfig` via
  `LaunchConfig::with_dir`, dialing the legacy per-`--dir` bridge daemon — not the global socket
  the other three clients use.
- `crates/txtodo-tui/src/daemon.rs:117-140` (`Daemon::wait_until_ready`) then connects to
  whatever that per-dir config points at, with its own independently-tuned retry budget
  (`CONNECT_TIMEOUT=500ms`, `MAX_CONNECT_RETRIES=5`, `RETRY_BACKOFF=150ms`, `daemon.rs:27-31`).
- Practical effect: a workspace with the global `txtodod` already running (started by `txtodo` or
  `apps/desktop`) still gets a second, separate per-dir daemon spawned when the TUI opens — two
  daemons for one workspace, and TUI-made changes may not be visible to other clients until sync
  catches up, instead of being immediately consistent through one shared process.

## Design

- Migrate `crates/txtodo-tui/src/app.rs::async_main`'s daemon-launch config from
  `LaunchConfig::with_dir` to the same global-socket `LaunchConfig` construction
  `crates/txtodo-cli/src/daemon_ensure.rs` and `apps/desktop/src-tauri/src/daemon/spawn.rs` use.
- Fold `Daemon::wait_until_ready`'s retry budget into (or have it explicitly defer to)
  `ensure_daemon`'s, so there's one tuned timeout/backoff policy for "wait for the daemon to be
  live," not two independently-guessed ones.
- Confirm the legacy per-`--dir` bridge daemon path is still needed for anything (e.g. a
  `--dir`-scoped debug/test mode) before deleting it outright — if nothing needs it, remove it
  rather than leaving dead code behind.

## Out of scope

- Any change to `crates/txtodo-daemon-launch` itself — this is a call-site migration in the TUI,
  not a change to the shared crate's behavior.

## Acceptance

- Opening the TUI when the global `txtodod` is already running (started by another client)
  connects to that same daemon — no second daemon process spawned.
- Opening the TUI with no daemon running spawns exactly one global `txtodod`, same as `txtodo`/
  `apps/desktop` do today.
