# Cross-workspace, agent-aware activity feed (storyboard screen 08)

## Goal

The left nav's `Activity` tab (scaffolded, empty, by `desktop-workspace-nav-sidebar`) shows one
rolled-up feed of the newest ops across *every registered workspace*, not just the one open in the
main view, with autonomous-agent edits visually distinct from a person's. Today's activity feed
(`apps/desktop/devices/ActivityFeed.svelte`, plan M7) is scoped to whichever single workspace the
daemon has open.

## Current state (read before writing anything)

- `apps/desktop/src-tauri/src/commands_activity.rs::op_log` — no arguments beyond `AppHandle`/
  `State`; calls `client.op_log()` which always uses `self.selector.clone()`
  (`apps/desktop/src-tauri/src/daemon.rs:325-336`), i.e. whatever workspace this `DaemonClient` is
  currently pointed at. One workspace per call, hardcoded.
- `crates/txtodo-proto/proto/txtodo/v1/txtodo.proto:505-513` — `OpLogRequest` already carries a
  `WorkspaceSelector workspace` field (added for every RPC by `daemon-global-socket`), but
  `OpLogEntry` itself carries only `principal`, `op`, `at_ms` — **no workspace identifier on the
  entry**. Aggregating client-side means tagging each entry with its source workspace ourselves;
  the daemon has nothing to add here.
- `OpLogEntryDto.principal` (`apps/desktop/src-tauri/src/dto_activity.rs:10`) is already documented
  and formatted as `"you@dev"` / `"agent:name@dev"` / `"external@dev"` — the agent/human distinction
  this task needs to render is already in the data, today. No daemon or proto change needed for that
  part either.
- `WorkspaceCatalog::resolve` (per `tasks/mcp-multi-workspace-gateway/notes.md`'s "Edge cases")
  **lazily opens an unknown-but-registered path or id on first use**. This is the key finding that
  keeps this task desktop-only: we don't need a daemon-side "list of currently open workspaces" —
  fanning `op_log_stream` out over every *registered* workspace (`listWorkspaces()`, already
  exposed) is enough; `resolve()` opens each one on demand as its selector is used.
- `WorkspaceCatalog::open_all_registered` (`crates/txtodo-daemon/src/workspace_catalog.rs:52-`)
  already establishes the precedent this task's error handling should copy: "per-workspace failures
  are logged and skipped — one bad workspace must not take the whole daemon down," and a registered
  root that no longer exists on disk is skipped silently.

## Design

### No daemon or proto changes

Everything needed already exists on the wire (`OpLogRequest.workspace`, `OpLogEntry.principal`) and
in the registry (`WorkspaceList`/`listWorkspaces()`). This is a Tauri-command + frontend task.

### New Tauri command: `op_log_all`

```rust
// apps/desktop/src-tauri/src/commands_activity.rs
#[tauri::command]
pub async fn op_log_all(app: AppHandle, state: State<'_, AppState>)
    -> Result<Vec<AggregatedOpLogEntryDto>, String>
```

- Calls `list_workspaces()`'s existing daemon RPC (`WorkspaceList`) to get every registered entry.
- Skips any entry with `root_exists == false` (mirrors `open_all_registered`'s silent-skip rule —
  there's nothing to fetch from a directory that's gone).
- For each remaining entry, issues one `op_log_stream` call with an explicit
  `WorkspaceSelector { workspace_id: entry.id }` (not the client's ambient selector — this is the
  one place `DaemonClient` needs a per-call override rather than its stored `self.selector`).
- A single workspace's call failing (daemon can't open it, transient error) is logged and skipped,
  not propagated — same "one bad workspace doesn't take the whole thing down" rule.
- Merge-sorts the combined results by `at_ms` descending, caps at 200 total (same bound as today's
  single-workspace feed), tags each entry with `workspace_root` (and `workspace_id`) before
  returning — this is the field `OpLogEntry` doesn't carry, added here, not on the wire.

### DTO

```rust
// dto_activity.rs — extends, doesn't replace, OpLogEntryDto
pub struct AggregatedOpLogEntryDto {
    pub principal: String,      // unchanged format, see above
    pub op: String,
    pub at_ms: u64,
    pub workspace_id: String,
    pub workspace_root: String,
}
```

### Frontend

- New component (or a mode on `ActivityFeed.svelte` — pick whichever avoids duplicating the
  relative-time formatting in `devices/time.ts`) rendered inside the nav's `Activity` tab.
- Per row: workspace chip (`workspace_root`, truncated), principal, relative time, then the op
  summary — matching the storyboard's layout.
- Agent distinction is a pure string check on `principal` (`starts_with("agent:")`) — render the
  existing small pill style; no new backend signal required.
- Same bounded-fetch contract as today's feed: refresh button + refetch on window focus, **not** a
  live tail. The storyboard's "agents working autonomously" framing makes a live stream tempting,
  but nothing on the daemon side currently pushes op-log events (`watch` streams file *content*
  changes, not op-log rows) — wiring a live tail is a separate, larger task if wanted later, out of
  scope here.

## Explicitly out of scope

- Any live/streaming variant of this feed (would need a new daemon-side push, not just a new pull).
- Filtering/searching the aggregated feed.
- Anything on the `Workspaces` tab or the sidebar shell itself — `desktop-workspace-nav-sidebar`.

## Test plan

- Real two-workspace daemon fixture (the same shape `mcp-multi-workspace-gateway`'s real-daemon
  proof used, or the desktop `e2e_bridge` binary's existing multi-op setup) — assert the merged
  result is sorted across workspaces by `at_ms`, each entry carries the right `workspace_root`, and
  a registered-but-missing-directory workspace is skipped without failing the whole call.
- Unit test for the agent/human split on `principal` string parsing.

## Acceptance

- `op_log_all` returns a correctly merged, workspace-tagged, capped feed across every registered
  (root-existing) workspace.
- Activity tab renders it with agent principals visually distinct from human/device ones.
- One unreachable workspace never blanks the whole feed.
