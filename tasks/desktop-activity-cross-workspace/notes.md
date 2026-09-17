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

## As built (2026-09-17)

- `DaemonClient::op_log_for(workspace)` (`daemon.rs`): explicit-selector sibling of `op_log`, both
  now delegate to a shared `op_log_with(Option<WorkspaceSelector>)`.
- `AggregatedOpLogEntryDto` (`dto_activity.rs`), `op_log_all` Tauri command
  (`commands_activity.rs`): fans `op_log_for` out over every `WorkspaceList` entry with
  `root_exists`, tags each result with `workspace_id`/`workspace_root`, merge-sorts by `at_ms`
  descending, caps at 200. A single workspace's fetch failure is logged (`op_log_all_workspace_
  failed`) and skipped, never propagated — split into `op_log_one_workspace` to stay under
  clippy's cognitive-complexity budget.
- `$lib/daemon.ts::opLogAll()` + `AggregatedOpLogEntry` type; `ActivityTab.svelte` (new component,
  not inlined into `WorkspaceSwitcher.svelte`) renders it in the nav's Activity tab: workspace chip
  (last path segment, full path on hover), principal (agent pill via `activityLogic.ts::isAgent`),
  relative time (`devices/time.ts`, reused not duplicated), op summary. Same bounded-fetch contract
  as `devices/ActivityFeed.svelte`: fetch on mount, refetch on window focus, manual refresh button,
  no live tail.
- `isAgent`/`shortRoot` split into `activityLogic.ts` (pure, no DOM/Tauri) for unit testing, same
  pattern `editPopoverLogic.ts` uses — `activityLogic.test.ts`, 6 cases.
- Real e2e coverage (`e2e/activity-tab.spec.ts`, 3 cases): own-workspace activity shows by default;
  a second real workspace registered through the UI's own Add form gets merged in and tagged with
  its own root once the Activity tab's fan-out opens it; a registered-then-deleted workspace is
  skipped without an error state or blanking the rest of the feed. Also wired `op_log_all` into
  `e2e_bridge` (`e2e_bridge/activity.rs`, split for the file-length budget, restating this task's
  own merge/sort/cap logic since `op_log_all_inner`'s signature is tied to Tauri's extractors).
- Also updated `workspace-switcher.spec.ts`'s tab-switching test: it asserted the old "coming soon"
  placeholder text, which this task replaced with the real feed.
- Registry-leak follow-up (see `desktop-workspace-nav-sidebar/notes.md` and the spawned task) bit
  again here: every real-daemon Playwright run still leaks workspace-registry entries into the
  real global registry and leaves the shared daemon process running. Cleaned up by hand again this
  session (`txtodo workspace remove` in a loop + kill the daemon PID) — not re-flagged as a
  separate task since the existing follow-up already covers it.
