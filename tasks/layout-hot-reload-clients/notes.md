# layout-hot-reload-clients

Found reviewing the last 100 commits (`18b37ea^..HEAD`) on 2026-09-21.

## Goal

The daemon hot-reloads `txtodo.toml` while a workspace is open
(`crates/txtodo-daemon/src/layout_reload.rs:19`, `watch_task.rs:128`). No client listens. Recorded
as a known gap when the `todo_file` line closed (`f99bf0d`); never ticketed until now.

## Design

The `daemon-change` stream is already subscribed on the desktop side
(`apps/desktop/src-tauri/src/commands.rs:206`). Refetching the layout on a change that touches the
layout file — or on every change batch, which is cheap — closes it. The TUI needs the equivalent.

## The gaps

- `apps/desktop/src/lib/components/MainView.svelte:76-94` fetches `workspace_layout` only on a
  workspace-root change. After `txtodo workspace layout --todo-file work.txt` (or a synced layout
  change) the running app keeps opening the old root list and composing ref dirs from the stale
  `refs_dir` until a switch or restart.
- `MainView.svelte:83-86` fires `workspaceLayout()` without awaiting or sequencing. Two workspace
  picks in quick succession (allowed as soon as `switchWorkspace` resolves,
  `WorkspaceSwitcher.svelte:112-126`) leave two fetches in flight; if the first resolves last the
  store holds the previous workspace's layout while the view shows the new root. `FileView` already
  has the idiom for this (`refreshSeq`, `FileView.svelte:328`).
- `apps/desktop/src/lib/components/QuickAdd.svelte:24-33,50` starts `rootPath` at `"todo.txt"` and
  its catch keeps the last known value. A capture typed before the first fetch resolves, or any
  capture after a failed fetch, appends to a file the main view never shows — the task looks lost.
  Await the refresh inside `handleSave`, or disable submit until the first answer lands.
- The TUI caches the root list for the session (`crates/txtodo-tui/src/daemon.rs:183-191`).

## Known gap

Nothing here is a data-loss bug on its own — the writes land in a real file, just not the one the
user is looking at. It reads as "my task vanished", which is worse than an error.

See [[layout-client-gaps]].

## As built (2026-09-23)

- Daemon: `SharedLayout::set` now notifies and the `Watch` RPC forwards it as a `Change` with
  path `txtodo.toml` (no hash/ops) on every stream, whatever documents were named.
- Desktop: `MainView` refetches the layout on that notice, sequenced (`layoutSeq`); `QuickAdd`
  asks for the layout right before each save and shows an error rather than writing to a guessed
  file. The e2e shim (`e2e/shim/event.ts`) polls `workspace_layout` and synthesizes the notice.
- TUI: `app_layout::follow_root_list` switches document and Watch stream when the root list moved.
- Found on the way: on macOS a workspace opened by a symlinked spelling (`/tmp/x`) never saw
  its watcher events (`/private/tmp/x/...`), so no hot reload and no external-edit pickup;
  `watch_task::under_root` now respells event paths onto the root.
- Not done: the desktop does not re-open the detail stack on a layout change (the root list
  remounts, the detail view keeps its old file path until the next navigation).
