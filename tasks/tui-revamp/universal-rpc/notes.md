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

## As built
