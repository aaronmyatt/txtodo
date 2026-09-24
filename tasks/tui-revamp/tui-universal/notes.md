# tui-universal

## Goal
- The c2 Universal page as a TUI screen.

## Design
- Data comes from the `UniversalTasks` RPC.
  - Grouping and due buckets use core `universal::*` against local today.
- Follow `apps/desktop/design-mockups/c2/universal.js`:
  - the stat strip and group selector
  - workspace chips (at least one stays on), show done, context chips
  - the row layout
  - the empty state with Reset filters
- Enter switches workspace, goes to Tasks and puts the cursor on that line.

## As built
- State and filtering (`state_universal.rs`): `UTask` mirrors `pb::UniversalTask`; `UniversalView::groups` filters (workspaces, context, done, the header query) and groups through core `universal::group`, so keys, mouse and drawing share one row list.
- Daemon side (`app_universal.rs`): `UniversalTasks(include_done=true)` on entry (`Action::RefreshUniversal`), on the 1 s tick while the screen shows, and after `x` / Undo.
  - `x` points the selector at the row's workspace for one `Apply`; the change records its workspace, so the toast's Undo goes there too.
  - Enter switches to the row's workspace by id (the open one too; switching re-reads it), then puts the cursor on its line by task id, else line number.
- Keys: j/k/Home/End, Enter, x, Tab (next grouping). `u` stays list mode only; Universal undoes through the toast.
- Known gaps: a workspace the daemon has not loaded is not listed (the RPC covers loaded ones); no Watch on other workspaces, so the tick is the refresh.
- Tests: `state_universal_tests.rs`, `commands_universal.rs`, `ui/universal_tests.rs`, and `tests/universal_screen.rs` against a real daemon.
