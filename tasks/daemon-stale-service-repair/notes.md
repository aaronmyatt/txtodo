# A broken persistent service install never self-heals

## Reported

Found 2026-09-18 while diagnosing a "Daemon: dead" desktop-app report. `launchctl list` showed
`com.txtodo.txtodod` with PID `-` and a nonzero last-exit (78): `KeepAlive` was retrying the same
launchd job forever, and it could never succeed, because
`~/Library/LaunchAgents/com.txtodo.txtodod.plist`'s `ProgramArguments` pointed at
`.../.claude/worktrees/desktop-ci-daemon-fixes-e93d88/target/debug/txtodod` — a binary inside a
git worktree that had since been deleted (`git worktree list` no longer shows it). Only running
`txtodo daemon install --force` by hand fixed it.

## Why it isn't already possible

`txtodo_daemon_launch::spawn::install_persistent_service_best_effort` (the code every
`ensure_daemon` caller relies on for "a reboot recovers this daemon") only ever *installs* with
`force: false` — "never clobber a service file a human or a previous install already customized"
— and if `service::install` fails because a file already exists, it just tries `start` on the
existing (possibly broken) file. Nothing ever inspects whether that existing file's
`ProgramArguments` still points at a real, executable binary. So a dev-checkout artifact (a
worktree's debug binary, deleted once the worktree is removed) can permanently wedge the one
persistent unit ADR 0025 says a device should have, with no client — CLI, TUI, MCP, or desktop —
ever noticing or fixing it. `ensure_daemon`'s own ad-hoc spawn still works around it for that one
process's lifetime (which is why the desktop app was, separately, not actually broken — see
`cli-global-socket-cwd-fallback`'s notes for how that got confusing), but every subsequent reboot
is back to a dead persistent unit until a human intervenes.

## What to reuse

- `crates/txtodo-daemon-launch/src/service.rs` — `render`/`install`/`start`, and whatever it
  already knows about reading back an installed unit's state (`crates/txtodo-cli/src/commands
  /service.rs`'s `daemon status` output — "installed" / exit code — comes from here or somewhere
  adjacent).
- `crates/txtodo-daemon-launch/src/spawn.rs::install_persistent_service_best_effort` — the one
  call site every client's `ensure_daemon` already goes through; the repair check belongs here so
  it's automatically shared, not duplicated per client.
- `launchctl print gui/<uid>/com.txtodo.txtodod` (macOS) / `systemctl --user show
  com.txtodo.txtodod` (Linux) — both can report the last exit status and, for launchd, the
  registered `ProgramArguments` — the two facts needed to decide "this unit is stale" without
  guessing.

## Design notes

- Two independent signals are enough to call a unit stale, and either alone might be a false
  positive (e.g. a real crash unrelated to the binary path): (1) the unit's recorded program path
  no longer exists on disk, or (2) it does exist but doesn't match the currently resolved
  `daemon_bin` *and* the unit's last exit was nonzero. Prefer signal (1) alone as sufficient —
  it's unambiguous — and treat (2) as supporting evidence, not sufficient by itself (a human may
  have deliberately pointed the unit at a custom binary).
- Repairing means the same thing `daemon install --force` already does: re-render and overwrite
  the plist/unit file, then (re)start it. Reuse that path rather than inventing a second one.
- This is a "best-effort, non-fatal" repair, same spirit as the install call it lives beside — a
  failure to repair should never block the ad-hoc spawn that already got the caller a working
  daemon for this process's lifetime.

## Acceptance

Every checklist item in `todo.txt`, plus: on a real machine, installing a service file that points
at a since-deleted binary, then launching any client (not just running `daemon install` by hand),
results in a working persistent unit afterward — `launchctl list` (or the systemd equivalent)
shows a live PID, not `-`/nonzero-exit.

## Not in scope

- The separate `cli-global-socket-cwd-fallback` and `daemon-mirror-assertion-panic` tasks, found
  during the same investigation but independent bugs.
- Detecting *every* possible way a service file could be wrong (e.g. a corrupted plist) — only the
  "points at a binary that no longer exists" case that was actually observed.
