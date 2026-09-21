# default-workspace-client-agreement

Found reviewing the last 100 commits (`18b37ea^..HEAD`) on 2026-09-21.

## Goal

The daemon reserves the default workspace under one id (ADR 0029). Two desktop code paths and the
e2e fixtures each *recompute its path* instead of asking for it, and the test that should have
caught the drift was weakened rather than strengthened.

## The gaps

- `apps/desktop/src-tauri/src/config.rs:31-37` (used at `lib.rs:67-69`) resolves the default
  workspace path from the *app's* process env; the daemon resolves it from *its own*. A
  launchd-managed `txtodod` — this repo ships one — with a different `XDG_DATA_HOME` or
  `TXTODO_SOCKET` than the user's GUI session makes the two disagree. Because the app connects with
  a Path selector, the daemon then auto-registers the app's guess as an ordinary workspace: the
  user sees two near-identical roots and the one they are in is not the one flagged "Default".
  Selecting by the reserved workspace id, or by the `is_default` entry from `list_workspaces`,
  cannot drift. (Confidence on the launchd scenario is low — worth reproducing before building.)
- `apps/desktop/src-tauri/src/bin/e2e_bridge.rs:80-82` and `apps/desktop/e2e/fixtures.ts:144` both
  hand-build `<globalDir>/default` while the comment points at
  `txtodo_workspace_paths::default_workspace_dir_for` as the real definition. If that layout
  changes, or `TXTODO_DEFAULT_WORKSPACE` enters the picture, the bridge names a directory the
  daemon has not reserved, the path selector auto-registers it as an ordinary workspace, and the
  `fresh` spec asserts "Default" against something that is not the default — a confusing failure
  rather than a clear one.
- `apps/desktop/src-tauri/tests/workspace_registry.rs:64-72,85-93`: the `305918b` fix is genuine —
  the daemon really does register a reserved default now
  (`crates/txtodo-daemon/src/default_workspace.rs:36`), so this is not a test taught to accept a
  bug. But `iter().all(|w| w.is_default)` is vacuously true on an empty list and passes with two
  defaults, so it no longer proves the add/remove round trip left exactly the default behind.
  `assert_eq!(listed.len(), 1)` plus `is_default` — the style already used at `:141-144` — restores
  the strength.

Default-workspace *removal* is genuinely guarded server-side
(`crates/txtodo-daemon/src/workspace_catalog.rs:150-155`), not just by the hidden button. That part
is fine.

See [[default-workspace-pairing-consent]].
