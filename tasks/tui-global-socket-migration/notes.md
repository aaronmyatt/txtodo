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

## As built (2026-09-18)

- `app.rs::async_main` now resolves `daemon::global_socket_path()` (delegating to
  `txtodo-workspace-paths`, `ref:daemon-paths-shared-crate`) and spawns via
  `LaunchConfig::new(&sock)` with no `.with_dir` — the exact shape `txtodo-cli`/`apps/desktop`
  already use.
- Found the real gap this migration needed to close, beyond the LaunchConfig swap alone: the
  global daemon can have several workspaces open at once, so every RPC needs a
  `WorkspaceSelector` telling it which one — `txtodo-cli`'s `client.rs` already had this
  (`selector: Option<pb::WorkspaceSelector>`, cloned onto every request); `daemon.rs::Daemon`
  gained the identical field, and `daemon::workspace_selector(path)` builds a `Path` selector
  (auto-registers/opens an unknown directory, `workspace_catalog.rs::resolve`, no separate
  `txtodo workspace add` step needed).
  `Daemon::connect`'s signature grew a `selector: Option<pb::WorkspaceSelector>` parameter to
  carry it, updated at all 5 call sites (`app.rs`, `daemon.rs`'s own 2 unit tests,
  `tests/roundtrip.rs`, `tests/support/mod.rs`) — the 4 test-harness ones pass `None`
  deliberately: they still spawn the legacy per-workspace bridge daemon (item 1's finding), which
  is unambiguous without a selector.
  - `daemon_autostart.rs` explicitly documents itself as replicating `async_main`'s exact
    behavior, so it needed rewriting too, not just a signature fix: it now spawns a hermetic
    *global* daemon (`TXTODO_SOCKET`/`TXTODO_REGISTRY_DB` overrides in `LaunchConfig::extra_env`,
    same pattern `apps/desktop/src-tauri/src/daemon/spawn.rs` uses for its own hermetic
    global-daemon tests) instead of a per-`--dir` one.
  - New test `a_second_workspace_reuses_the_already_running_global_daemon`: runs the
    ensure-then-connect sequence twice against one hermetic global socket for two different
    workspaces, asserts the pid file names the identical process both times (no second spawn),
    and that each `Daemon`'s `get_file` returns its own workspace's content, not the other's —
    directly proving both acceptance bullets at once, not just the "no double-spawn" half.
- One sub-item left open, not silently dropped: folding `wait_until_ready`'s own retry budget
  (`CONNECT_TIMEOUT`/`MAX_CONNECT_RETRIES`/`RETRY_BACKOFF`, `daemon.rs:27-31`) into or against
  `ensure_daemon`'s is unchanged — the design note flagging it as "only safe because
  `ensure_daemon` already blocked until live" still holds, so this is redundant-but-harmless, not
  a correctness gap; left for a follow-up rather than risking a hasty change to it this pass.
- `cargo build/clippy/test -p txtodo-tui` (48 unit tests + all 5 real-daemon integration suites)
  and `check-boundaries.sh` all green. Committed as `d73ba00`.
