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

## Design (proposed, pending the decisions in todo.txt)

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
- Still waiting: identity across devices (reserved id or offer/accept) and what no `--dir` means.

## Open

- Case-insensitive filesystems (macOS default, Windows) against case-sensitive Linux: `ref:` slugs
  are lowercase, so folders should not collide, but the audit line proves it.
- What an existing user's first launch does when they already have workspaces registered: the
  default is added beside them and is not auto-selected over their last pick.
- Moving the default's directory later is out of scope.
