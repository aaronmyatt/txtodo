# desktop-universal-view

## Summary

Desktop: universal view aggregating tasks (and their `ref:` sub-lists/notes) by priority and
`@context` across every registered workspace, breadcrumb shows the owning project.

## As built

`commands_universal::universal_tasks` loops `workspace_list()` + a new
`DaemonClient::get_file_for(selector, path)` (explicit override, never touches the client's own
selector) and parses each `todo.txt` with `txtodo-core`; a broken workspace is skipped, not fatal
(backend `b5f41fb`, real-daemon test in `universal_view.rs`). The `/universal` route groups by
priority and filters by `@context` chip toggles; clicking a task switches workspace and opens
straight to that line via a small `pendingUniversalNav` store `MainView`'s switch-effect consumes;
`DetailParams` grew an optional `workspaceRoot` so `Breadcrumb` shows the owning project on the
entry crumb (frontend `2b01744`).

**Deliberate scope cut, not done**: nested `ref:` sub-lists/notes are not aggregated — root
`todo.txt` only.
