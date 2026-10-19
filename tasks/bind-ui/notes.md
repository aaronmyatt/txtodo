## Goal

`txtodo workspace bind <remote-id>` is CLI only (see tasks/spike-txtodo-should-merge-remote-workspa).
Give the TUI and the desktop the same move: spot a folder whose list shares its name with a
Remote one, and offer "use the synced list here" in one confirmed step.

## Design (2026-10-19, agent)

- One pair finder, not three. Lift doctor_bind.rs's name match (own, non-default workspace vs
  Remote, same trimmed lowercase name) into a pure fn in txtodo-proto, which cli, tui and the
  desktop's Tauri side all depend on. doctor calls it too.
  - Rejected: a `bind_candidate` field the daemon fills on WorkspaceInfo. Cleaner wire story, but
    a daemon change and a daemon reinstall for a UI hint. Revisit if MCP wants it.
  - Rejected: a TS copy in the desktop. The paths code drifted 4x that way (daemon-paths-shared-crate).
- Always dry-run first. Both UIs call WorkspaceBind with dry_run=true, show what moves and the
  backup folder, then call again with dry_run=false on confirm. No new RPC.
- After a bind the folder's old id is gone. The client must switch its selection to the Remote id
  (now at the folder), or it is left pointing at a removed workspace.
- Show `done_text`'s hint: the other device still mirrors the old id; remove it there by hand.

### Desktop

- Tauri command `workspace_bind(workspace_id, root, dry_run)` over the RPC.
- Hint: in the sidebar (WorkspaceSwitcher.svelte) and Settings → Workspaces, an own workspace with
  a same-name Remote shows "Synced copy on another device — use it here". Opens a confirm dialog
  with the dry-run list; Bind runs it.
- Later, separate line: "Use in folder…" on any Remote row (native folder picker) for a folder
  whose name does not match.

### TUI

- Hint in the workspace picker row (same text, shorter), and a header note when the open workspace
  has a match.
- Key on that row opens a confirm modal (ui/offers.rs is the modal precedent) with the dry-run
  text; y binds, Esc cancels.

## Open questions

- None that block. Folder picker for non-matching names is deferred, not decided against.
