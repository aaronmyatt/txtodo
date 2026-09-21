# default-workspace

## Goal

Every user has one default workspace: the global todo list. txtodo creates it on first run in a
directory it manages, so the app and the CLI always have somewhere to write, and it syncs across
that user's devices like any other workspace. Decided 2026-09-20; it replaces both options of the
old "Decide: apps/desktop with no workspace selected" line (refuse writes, or auto-pick the sole
workspace).

## Functional requirements

- First run creates it. There is nothing to configure or pick.
- It is a normal workspace: a `todo.txt`, `ref:` folders, notes, history, undo. It appears in the
  workspace list, marked as the default.
- It cannot be removed, only hidden by picking another.
- A call that names no workspace lands in it (desktop quick-add with nothing picked, `txtodo add`
  outside any workspace, MCP with no selector).
- It syncs to every paired device and over the relay. On each device it lives wherever that OS
  keeps txtodo's data (macOS and Linux `~/.local/share/txtodo`, Windows `%LOCALAPPDATA%`), and the
  paths never need to match: devices agree on identity, not location.
- Created empty. A seed task would be added once per device and come back from sync as duplicates.
- Two devices that each already have tasks in their default converge to the union of both.

## Design (the three decisions below are made, 2026-09-20)

- Identity: a reserved `WorkspaceId` (non-zero ULID timestamp, unlike the all-zero link sentinel)
  that every device registers with `WorkspaceRegistry::adopt` for its default. Sync frames are keyed
  by workspace id, so two devices with the same id already exchange ops; no offer to accept, no rekey.
  Different unrelated users share the constant harmlessly, since they never sync together.
- Location: `default_workspace_dir(env)` in `txtodo-workspace-paths`, next to `data_dir`, with an
  env override for tests.
- Selector-less resolution: `WorkspaceCatalog::resolve_sole_open` prefers the default when it is
  registered. The `--dir` bridge is unchanged.
- Loading: the default is queued first by the early-bind loader, so the desktop is usable at once.
- Nothing synced holds a path: `FilePath` is workspace-relative with `/` separators, and offers
  carry a name. The audit line checks bundle export and notes too.

## Decided

- 2026-09-20, location: A. The default workspace lives under txtodo's own data dir
  (`.../txtodo/default`, beside `registry.db`), resolved per OS by `txtodo-workspace-paths`. Not a
  visible folder in the home directory. Cost accepted: Finder does not show it, and cleaning
  `~/.local/share` by hand would delete real tasks, so `txtodo workspace default` prints the path
  and `doctor` reports it. This unblocks the `workspace-paths: default_workspace_dir(env)` line.
- 2026-09-20, identity: A. One reserved `WorkspaceId`, a constant in the code, that every device
  registers its default under (`WorkspaceRegistry::adopt`). Sync frames are keyed by workspace id,
  so two paired devices sync their defaults with no offer to accept and no rekey. Unrelated users
  share the constant, which is harmless: they never pair. Cost accepted: the constant is
  permanent, and changing it later is a migration, so pick it once (a non-zero ULID timestamp, so
  it can never be the all-zero link sentinel) and record it in the ADR line. The registry line
  needs no new column now.
- 2026-09-20, no `--dir`: A. The CLI, TUI and MCP use the current folder when it is a workspace,
  else the default workspace. Today's habit keeps working (`cd` into a repo, `txtodo add` writes
  there), and outside any workspace a command lands in the default instead of failing. Cost
  accepted: the same command writes to different places depending on where you stand, so a client
  that fell back to the default should say so. "Is a workspace" needs one definition shared by the
  three clients; the client line owns it.
- All three decisions are made. Nothing in `todo.txt` waits on a human now, except the ADR's
  wording.

## Open

- Case-insensitive filesystems (macOS default, Windows) against case-sensitive Linux: `ref:` slugs
  are lowercase, so folders should not collide, but the audit line proves it.
- What an existing user's first launch does when they already have workspaces registered: the
  default is added beside them and is not auto-selected over their last pick.
- Moving the default's directory later is out of scope.
- CI regression found 2026-09-21 (line 17): `apps/desktop/src-tauri/tests/workspace_registry.rs`'s
  `add_list_remove_round_trip_and_add_is_idempotent` and
  `an_unbound_client_is_ready_and_lists_an_empty_registry` both assert
  `client.workspace_list().await.unwrap().is_empty()` on a fresh client — true before this task,
  false now that every daemon always registers the default. Broke in f73d31e9's own CI run
  (2026-09-20) and has stayed red since (confirmed still failing on 2026-09-21's push). Not
  covered by any line above; those are new tests for the feature, not a fix for these two old
  ones.
