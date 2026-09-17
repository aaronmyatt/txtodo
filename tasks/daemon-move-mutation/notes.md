# daemon-move-mutation

## Summary

Move mutation: relocate a task's line (e.g. completed items to the bottom) without minting a new
id, so `ref:` notes/sub-lists survive.

## As built

Already implemented: `OpKind::Move`'s `after` anchor already covers same-file relocation
(`state.rs::move_task` branches to in-place remove+reinsert at the same id when
`to_file == file`); `mutation.rs::MoveToEnd`/`move_to_end_ops` wires "completed items to the
bottom" specifically (used by `txtodo archive` and MCP `todo_archive`), asserted by
`tests/m5_acceptance.rs::archive_line_one` that `ref:` survives. Landed in `a0dfaef`
(2026-09-13, removing the old `done.txt` convention).

**Only gap**: no client-facing "move to arbitrary same-file position" `Mutation` variant (only
`MoveToEnd`) — the underlying primitive supports any anchor already; a thin follow-up only if
actually wanted, not separately tracked.
