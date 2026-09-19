# Tests leak temp workspaces into the real registry

## Goal

The desktop Workspaces tab showed ~130 dead `/private/var/folders/.../T/...` roots. Stop new
leaks, and give a way to clear old ones.

## Found 2026-09-19

- `~/.local/share/txtodo/registry.db`: 133 rows, 24 active, 124 under the macOS temp dir.
- 108 rows are `txtodo-e2e-*` (Playwright desktop e2e), added 2026-09-17 21:17-21:46 +0800.
  Fixed by `0e59c22` (e2e daemon now uses its own `TXTODO_E2E_GLOBAL_DIR`).
- 17 active rows are `.tmpXXXXXX` and `.tmpXXXXXX/q4-roadmap`, added 2026-09-18 01:22-01:34
  +0800 (a real daemon was running; its log shows `watch_drain_started` for each root).
  Source test NOT identified.
- Detector run at HEAD: every crate's tests (cli, mcp, tui, daemon-launch, daemon, desktop) run
  with `XDG_DATA_HOME` at a scratch dir left it empty. So no test at HEAD reaches the default
  socket/registry. The `.tmp` leak was older code or another worktree (`.claude/worktrees/*`).
- Removing a workspace only sets `removed_at`; rows are never deleted, so dead roots pile up.
  `list_registered` already computes `root_exists`, but the desktop list ignores it.
- Side finding: daemon-launch, tui and desktop test runs print `Bootstrap failed: 5` from
  `launchctl`. Harmless here, but tests should never talk to the real launchd domain.

## Not done

- Nothing in the real registry was changed. Clearing the 124 dead rows is a human step.
