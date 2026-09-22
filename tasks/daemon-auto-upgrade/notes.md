# daemon-auto-upgrade

## Goal

A client (CLI, TUI, MCP, desktop) that finds a running daemon older than itself restarts that
daemon with the newer binary, so an upgrade of the binaries upgrades the running daemon without
`txtodo daemon install --force` by hand.

## Why

Root line added 2026-09-22 after the version-info work: an installed app spawned a bundled
`txtodod` from before early bind and every client kept talking to it. `txtodo doctor` now warns
(task version-info) but nothing acts on the warning.

## Design

- The running daemon's version comes from a file, not gRPC: `txtodod` writes `txtodod.version`
  (its `CARGO_PKG_VERSION`) beside `txtodod.pid` at startup. `txtodo-daemon-launch` may not
  depend on `txtodo-proto` (`allowedDeps`), and reading a file keeps the check inside
  `ensure_daemon`, the one call every client already makes.
- `LaunchConfig.upgrade_to: Option<String>` is the client's own version. `ensure_daemon`, when
  the socket is live and `upgrade_to` is newer than the file's version, restarts the daemon:
  - Only the global shape (no `--dir`), only when the resolved `txtodod` binary itself reports a
    version newer than the running one (`txtodod --version`), so a stale `$PATH` copy never
    causes a restart loop and nothing ever downgrades.
  - With a loaded service unit owning the socket: `service::stop`, `service::install(force)`
    (repoints the unit at the new binary), `service::start`.
  - Otherwise (ad-hoc daemon): SIGTERM the pid in `txtodod.pid`, but only if the pid file's lock
    is still held (a dead daemon's stale pid is never signalled), wait for the socket to close,
    then the normal spawn path.
  - A missing version file beside a locked pid file reads as "older than this feature", so the
    first upgrade to a build with this feature also works. A missing pid file means it is not a
    `txtodod` state dir: do nothing.
- Honors `TXTODO_NO_AUTOSTART=1` (a client that may not spawn may not restart either).
- Version comparison is a numeric `major.minor.patch` triple; anything unparsable compares as
  "do nothing".

## Rejected

- Comparing `Health.version` in each client: four copies of the check, and the CLI has no
  Health call on its common path.
- Assuming the sibling binary matches the client version without running `--version`: a `$PATH`
  `txtodod` behind a newer CLI would restart the daemon every command and still be old.

## Known gaps

- A restart drops every other client's `Watch` stream; the TUI reconnects (bounded), the desktop
  reconnects through its own status loop. Same effect as `txtodo daemon install --force` today.
