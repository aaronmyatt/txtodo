# sidecar-task-ids

## Goal

Under Sidecar identity a line has no `id:` tag, so a client that reads the id out of the line text
finds nothing. Two places do that today:

- `apps/desktop`: `DetailView.svelte` (`/\bid:(\S+)/`) sets `parentTaskId` to `""`, so `GetNotes`
  and `EditNotes` fail (`parse_required_task_id`). `rawMode.ts` uses the same regex to keep a
  line's identity across a raw edit.
- `txtodo-mcp`: `parse::parse_row` and `parse::find_by_id` read `id:` from the text, so
  `TaskRow.id` is always empty and every id-addressed tool (`todo_get`, `todo_complete`,
  `todo_edit`, `todo_delete`, `todo_move`, `todo_notes_*`) answers "no task id:...".

The daemon already knows each line's id (`DocState` holds an `Entry::Task { id, .. }` per line).

## Design

- `FileContents.task_ids` (field 4, `repeated string`): one entry per line of `bytes`, in file
  order. ULID text for a task line, `""` for a blank line. Additive: an old client ignores it, an
  old daemon sends none and the clients fall back to the `id:` regex (so a Tagged workspace on an
  old daemon still works).
- Daemon: the actor's `Get` reply carries the ids next to the bytes, both read from the same
  actor turn, so they always describe the same lines. `Checkout` (a historical render) sends none.
- MCP: `get_file_text` becomes a `FileDoc { text, task_ids }`; rows take the daemon's id first and
  the text tag only as the fallback. `find_by_id` looks in the ids first.
- Desktop: `FileContentsDto.task_ids` to the frontend; `DetailView` takes the id of line N from it
  (`todotxt/taskIds.ts::taskIdAt`), the regex stays as the fallback. `rawMode.ts` is left alone:
  its problem is matching an edited buffer line to a baseline line, which ids on the baseline do
  not solve. That is `tasks/desktop-reorder-propagates` (save with `Replace`).
- Slice fence: proto, daemon, mcp, desktop land as four commits, in that order.

## Rejected

- A new `TaskIds` RPC: a second call could describe different lines than the `GetFile` before it.
- Ids on `Change`: every client re-reads with `GetFile` after a `Change`, so it is not needed.
- Line-number-only addressing in MCP: line numbers move under an agent between two calls; the id
  is the point of these tools.

## Known gaps

- The CLI and the TUI still address by line number plus `RequireBase`. Not part of this line.

## As built (2026-09-20)

- proto `FileContents.task_ids` (field 4) and the regenerated code: 2 commits before `c7b0e8e`.
- daemon `c7b0e8e`: `contents.rs` (`Contents.task_ids`, from `DocState::line_ids`, same actor turn
  as the bytes). `tests/get_file_task_ids.rs`: Sidecar and Tagged, and the id drives `Apply` and
  `EditNotes`.
- desktop `f193dc2`: `todotxt/taskIds.ts::taskIdAt`; `DetailView` notes use it.
- mcp `43a94f2`, `943774d`: `doc.rs::FileDoc`. When the daemon sent ids, a leftover `id:` word in
  the text is a plain tag (it lands last in `kv`), not the identity.

Still broken or not proven:

- The installed daemon (launchd, `~/.cargo/bin/txtodod`) is the old build. Until it is reinstalled
  it sends no `task_ids`, and both clients fall back to the `id:` regex, so nothing changes for this
  repo's own Sidecar workspace yet.
- Desktop notes under Sidecar were not looked at in the running app.
- The MCP real-daemon test is `#[ignore]`d (it needs a built `txtodod`), so CI does not run it.
- `locate_by_id` still reads every todo file of the workspace to find one id. Slow on a big
  workspace; a daemon-side "where is task X" RPC would fix it. Not part of this line.
