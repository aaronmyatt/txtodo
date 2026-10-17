# Move or copy a task to another list

## Goal

Move a task, or copy it, from one list to another list in the same workspace (the root
`todo.txt`, any `tasks/<slug>/todo.txt`), from the desktop, the CLI and MCP.

## What is there (2026-10-17, agent triage)

- The daemon has a cross-file move: a lone `Mutation.Move { task, to_path }`
  (`apply_route.rs` → `move_coordinator.rs`). The source records `OpKind::Move`, the destination
  an `Insert` of the same line at its end, and the task's `ref:` dir moves beside the destination
  (slug collision → `-2`), with rollback.
  - Gap: the collision rewrite and the rollback find the arrived line by its `id:` tag
    (`line_number_of`). Sidecar, the default, has no tag, and the destination minted its own id,
    so both fail there. No sidecar test.
- CLI `mv ITEM# DEST [SRC]` edits files directly (blank the source line, append to DEST) and never
  moves the ref dir. In daemon mode it runs in the scratch copy, which holds only the root list,
  so it fails: "Destination file … does not exist".
- MCP `todo_move` only reorders inside one file.
- The desktop has no move or copy at all; its bridge already knows `Move` (`dto.rs`,
  `daemon.ts`), and `listFiles()` gives the lists to pick from.
- Copy exists nowhere. An `Add` at the destination path is all it needs.

## Decided 2026-10-17 (agent)

- **Copy is a new task**: the same line at the end of the other list, minus `id:` and `ref:`. Its
  notes and sub-list stay with the original. Rejected: copying the ref dir too (two folders, and
  which notes are the real ones?); a new `Copy` mutation (a proto change for what `Add` does).
- **Move keeps the task**: always the daemon's `Move`, so history, the ref dir and its notes go
  with it. The CLI's `mv` stops editing files in daemon mode.
- **Desktop**: "Move to…" and "Copy to…" in the line's detail, picking from the workspace's
  lists; list-mode keys `m` and `c` open the same picker. Manifest rows `list.move_to` and
  `list.copy_to`; the TUI gets an `@parity` line.
- **MCP**: `todo_move` takes `to_file` (move to the end of that list). Copy is `todo_get` then
  `todo_add` with `file`; the tool text says so. No new tool.
- Out of scope: another workspace (it would be an `Add` there and a `Delete` here, and nothing
  ties the two); a list that does not exist yet.
