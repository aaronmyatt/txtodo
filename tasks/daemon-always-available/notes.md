# daemon-always-available

## Goal

Every client (`txtodo` CLI, `txtodo-tui`, `txtodo-mcp`, `apps/desktop`) optimistically ensures the
device-global `txtodod` is reachable — spawning it if absent, the way `apps/desktop` already does —
and the daemon survives a reboot without a manual `txtodo daemon start`. Decided 2026-09-17
(human), superseding `tasks/tui-daemon-autostart`'s open start-mechanism question
(`id:01M2Q9TUIDAEMONDECIDE00000`, now closed with a note pointing here): don't pick ad-hoc-spawn
*or* installed-service, do **both** — ad-hoc spawn covers "not running right now", auto-installing
the persistent service covers "won't come back after reboot".

## Current state (read before writing anything)

- `apps/desktop/src-tauri/src/daemon/spawn.rs::ensure_daemon` — the only client that auto-spawns
  today. `probe_live`/`SpawnGuard`/`wait_until_live`/`spawn_txtodod`, Unix-only (`#[cfg(unix)]`,
  stub elsewhere). Desktop/Tauri-specific code, not reusable as-is.
- `crates/txtodo-tui/src/app.rs` — deliberately does not spawn (own doc comment cites design §7);
  errors and exits on a dead socket. Tracked, decision now resolved, by
  `tasks/tui-daemon-autostart` (its remaining wire/UX/test/docs lines still apply — see item 3
  below, which supersedes only its "mirror spawn.rs" approach, not the work itself).
- `crates/txtodo-cli` — `client::select()` (`crates/txtodo-cli/src/client.rs:85-107`) silently
  falls back to file-direct mode when no socket exists, so file-only commands (add/list/etc.) never
  needed a daemon. Daemon-required commands (history/blame/undo/checkout/conflicts/pair/workspace/
  device/bundle, `main.rs:174-216`) error with `NEEDS_DAEMON` (`commands/history.rs:14` and
  siblings) instead of trying to start one.
- `crates/txtodo-mcp` — pure gRPC client (`grpc_backend.rs:63-89`, `main.rs:118-134`), errors "is
  txtodod running?" if the socket refuses connection. No spawn logic.
- `crates/txtodo-cli/src/commands/service.rs` — `txtodo daemon install|start|stop|status` is
  **already fully built**: renders `deploy/launchd/com.txtodo.txtodod.plist`
  (`RunAtLoad=true`/`KeepAlive=true`) or `deploy/systemd/txtodod.service`
  (`WantedBy=default.target`/`Restart=on-failure`), registers via `launchctl bootstrap`+`kickstart
  -k` or `systemctl --user enable --now` (`service.rs:207-227`). This is exactly the reboot-survival
  mechanism — it's just never invoked automatically by anything today.
- No shared crate exists for spawn-if-absent logic. `crates/txtodo-mcp/src/global_socket.rs`'s own
  doc comment says it can't depend on `txtodo-daemon` (`budgets.json`'s `allowedDeps` enforces the
  dependency direction) — a *new*, dependency-light leaf crate (just `Command::spawn` + a UDS probe,
  no `txtodo-daemon` dependency) avoids that constraint.
- ADR 0025 (`docs/adr/0025-global-daemon-one-process-per-device.md`) settles daemon *topology* (one
  `txtodod` per device) but takes no position on spawn-vs-service lifecycle — that decision was
  open until this task.

## Design

1. **Extract a shared spawn-helper crate** from `apps/desktop/src-tauri/src/daemon/spawn.rs`'s
   `ensure_daemon`/`probe_live`/`wait_until_live`/`SpawnGuard` shape — new leaf crate (name TBD),
   `#[cfg(unix)]` + stub, zero `txtodo-daemon` dependency so `txtodo-mcp` can use it too.
2. **Extend it**: on the branch that actually spawns `txtodod`, also install+enable the persistent
   boot service (call into or duplicate the minimal logic from
   `crates/txtodo-cli/src/commands/service.rs::install`/`start`) — so after the first ad-hoc spawn
   on any client, subsequent reboots recover via the installed unit, not another ad-hoc spawn.
   Idempotent: a second `install`/`start` on an already-installed service must be a no-op, not an
   error.
3. Wire `crates/txtodo-tui/src/app.rs::async_main` to call the shared crate when
   `wait_until_ready` fails — this is `tasks/tui-daemon-autostart`'s remaining wire/UX/test/docs
   lines (2-5 in its `todo.txt`), now implemented via the shared crate instead of a TUI-local mirror
   of `spawn.rs`. Don't duplicate those lines here; close them from there when this ships, cross-
   referencing this task.
4. Wire `crates/txtodo-cli`'s daemon-required commands (the `NEEDS_DAEMON` path) to attempt
   ensure-then-retry instead of erroring immediately.
5. Wire `crates/txtodo-mcp`'s connect path (`grpc_backend.rs`/`main.rs`) the same way.
6. Desktop migrates its own `ensure_daemon` call site to the shared crate too — dedupes the logic
   ADR 0025's consequences section already flagged as duplicated; lower priority, desktop's own copy
   already works correctly.
7. Tests: a cold-start integration test per client (no daemon running, no service installed) ends
   up connected without manual intervention; a "simulated reboot" case (service installed, process
   killed) recovers via the installed unit rather than triggering another ad-hoc spawn.
8. Docs: fix `crates/txtodo-tui/CLAUDE.md`'s stale "the TUI never spawns the daemon" line and any
   equivalent claims in cli/mcp docs.

## Explicitly out of scope

- `ref:desktop-daemon-sidecar-bundle` — getting `txtodod` onto the machine in the first place for a
  packaged desktop install. This task assumes the binary already exists somewhere reachable; that
  one is about distribution.
- `ref:desktop-cold-boot-dead-status` — desktop's own status-reporting UI bug, unrelated to spawn
  mechanics.
- Windows support for the spawn-helper crate — matches today's `apps/desktop` scope
  (`#[cfg(unix)]`, stub on other platforms); a separate task if Windows daemon lifecycle is prioritized.

## Open questions for the human (tagged `@human` on the relevant lines)

- Exact shared-crate boundary/name and its dependency graph (item 1).
- Whether the auto-install-on-spawn step (item 2) needs a `--no-daemon`/opt-out escape hatch per
  client (matches the CLI's existing `--no-daemon` flag; TUI/MCP have no equivalent today) — a
  human call on whether "always available" should ever be skippable.
