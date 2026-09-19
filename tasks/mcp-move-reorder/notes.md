# mcp-move-reorder

## As built (2026-09-20)

- New `Mutation.MoveBefore { task, before }` (proto 4b4a14b): the same-file relocation `MoveToEnd`
  is the "after the last task" case of. The daemon anchors the task after `before`'s predecessor
  (`mutation_moves.rs::move_before_ops`), or anchors to the task's own predecessor when it already
  sits there, so the reorder is a true no-op.
- `todo_move {id, before|after}` (`crates/txtodo-mcp/src/grpc_move.rs`): `before` is `MoveBefore`;
  `after` is `MoveBefore` the next non-blank task, `MoveToEnd` when the anchor is last, nothing when
  the task already follows it.

## Known gaps

- A blank line between the two tasks stays where it was, so "before B" can land above a blank that
  sat above B.
- Ids come from `id:` tags in the text, so a Sidecar workspace (no tags in the file) cannot be
  addressed by id yet; that is the existing limit of every id-keyed MCP tool.
