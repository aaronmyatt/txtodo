# tui-daemon-autostart

## Reported

`txtodo-tui` with no daemon running prints:
```
daemon not running — run `txtodo daemon start` (.../txtodod.sock: daemon rpc: ... No such file or directory)
```
and exits, instead of starting the daemon itself.

## Why it's not just a bug

`crates/txtodo-tui/src/app.rs`'s own doc comment on `async_main` says this is deliberate:

> The daemon is never spawned by the TUI itself (design §7 edge case: "the TUI never spawns the
> daemon, the desktop shell does").

That exact sentence isn't in `txtodo-design.md` verbatim (grepped, not found) — it's recorded only
as this file's own doc comment, so the rationale behind it isn't otherwise written down anywhere
this session could find. Reversing it is a real architectural call, not a one-line fix.

## What already exists to build on

- `apps/desktop/src-tauri/src/daemon/spawn.rs::ensure_daemon` — probe-live / client-side flock /
  spawn `txtodod` / poll-until-live. Unix-only today (`#[cfg(unix)]`, stub on other platforms).
- `txtodo daemon start` CLI subcommand (plan M3, `tasks/daemon-service-files`) — shells to
  `launchctl`/`systemctl --user` against a rendered launchd/systemd unit. Assumes the service was
  already `install`ed once.

## Open question for the human

Two different notions of "start the daemon":
1. **Service-manager start** (`txtodo daemon start`) — assumes `install` already ran; correct if
   the user is expected to have set up the service once.
2. **Ad-hoc spawn** (`ensure_daemon`'s shape) — spawns `txtodod` directly, no service manager
   involved; correct for a zero-setup "just works" experience but means the TUI now owns a process
   lifecycle it didn't before, and needs the same client-side lock story on Windows too (spawn.rs
   is unix-only).

Picking between these (and whether both need a `--no-daemon`/opt-out escape hatch, matching the
CLI's own flag) is the first sub-task, flagged `@human`.

## Resolved and superseded (2026-09-17/18)

Human decided: do **both** (ad-hoc spawn covers "not running now", auto-installing the persistent
launchd/systemd service covers "won't come back after reboot") — neither option alone from the
question above. Scope was broadened past just the TUI to every client (`txtodo`, `txtodo-tui`,
`txtodo-mcp`, `apps/desktop`), so the remaining implementation work (wire app.rs, failure-path UX,
tests, docs) now lives under `tasks/daemon-always-available` instead of here, to avoid tracking
the same work in two places. This ref's own todo.txt lines are closed as redirected, not
implemented — see `tasks/daemon-always-available/notes.md` for the actual design and remaining
work.
