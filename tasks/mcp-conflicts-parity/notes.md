# mcp-conflicts-parity

## As built (2026-09-20)

- `todo_conflicts_list {file?, workspace?}` and `todo_conflicts_resolve {id, side, file?, workspace?}`
  (`crates/txtodo-mcp/src/grpc_hygiene.rs`) call the daemon's `ListConflicts` / `ResolveConflict`.
- `ResolveRequest` gained `agent` (proto 18e61cb); the daemon stamps it (1a1f472), so an agent's
  resolution is attributed to that agent in the op log. Unset still means the human user, which is
  what the CLI, TUI and desktop send.

## Known gaps

- The list returns both texts (`mine`, `theirs`), not a unified diff; the client can diff them.
- `todo_conflicts_resolve` finds the line from the flag, so a task with no open flag is `not_found`.
