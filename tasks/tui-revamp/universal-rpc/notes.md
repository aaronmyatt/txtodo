# universal-rpc

## Goal
- One daemon RPC gives every client the cross-workspace task rows.
  - It replaces desktop's client-side `apps/desktop/src-tauri/src/commands_universal.rs` and covers the rich data in revamp line 33.

## Design
- `UniversalTasks(include_done)`. Each row carries:
  - workspace id and name, line_number, task_id, raw
  - done, completion_date, priority, due, projects, contexts
  - ref n/m, has_notes
- It reads root lists only, same as today.
- The daemon sends the raw due date. Clients bucket it with core `universal::due_bucket` against their local today (ADR 0011).
- Reopen waits on the human decide line in `../todo.txt`: a new mutation, or `Complete{undo}`.
- Proto changes and the daemon handler are separate fenced sessions.

## As built (2026-09-25)
- Proto (d9d1695): `UniversalTasks(UniversalTasksRequest{include_done}) -> UniversalTasksResponse`,
  `UniversalTask` as designed plus `has_ref` beside `ref_progress` (a `ref:` whose sub-list does not
  exist yet has no progress to send).
- Daemon: `universal_grpc.rs`, `WorkspaceCatalog::ready`; `workspace_name` is `default` for the
  default workspace, else the root's folder name (a mirror's folder is its workspace id: no human
  name is stored for it).
- Test: `universal_grpc_tests.rs`.

## Known gaps
- Reopen waits on its decide line.
- Desktop still builds Universal client-side (`commands_universal.rs`); switching is an `@parity` line.
