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

## As built (2026-10-17)

- daemon 490ec4ed: a cross-file move peeked the line by its `id:` tag, so on Sidecar every one
  failed ("no line N"); it reads the actor's id list now, and the slug rewrite and the rollback
  find the arrived line by id. 600fbefc: the arriving half is `Mutation::Arrive` (internal, never
  on the wire), so the task keeps its id in the new list on Sidecar too (an `Add` minted a new
  one, which lost its history). `add_ops` moved to `mutation_add.rs`.
- cli d3fbad37: `mv` in daemon mode is the daemon's `Move` (it used to fail on the scratch copy);
  `cp ITEM# DEST [SRC]` in both modes, dated today when undated.
- mcp f458521e: `todo_move` `to_file`; `todo_batch`'s move too. No `todo_copy`: the tool text says
  `todo_get` then `todo_add` with `file`.
- desktop ecd1ddbb, 805a2bed: "Move to…" / "Copy to…" pickers in the detail; `m`/`c` in list mode
  open the detail with that picker focused. Manifest rows `list.move_to`, `list.copy_to`.
- Tests: daemon Sidecar unit tests (rewrite, rollback, same id), CLI e2e (`move_copy.rs`), MCP e2e
  (`sidecar_id_tools.rs`), desktop e2e (`move-copy.spec.ts`).
- Known gaps: a move is not on the desktop's `u` stack (its Undo would bring back only the
  source half); the TUI has no move/copy yet (root `@parity` line); no move to another workspace;
  the destination must be a list the daemon already holds.
