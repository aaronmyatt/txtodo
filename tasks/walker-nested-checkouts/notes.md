# walker-nested-checkouts

## Goal
A workspace's walker does not descend into another git checkout nested inside it.

## Evidence (2026-09-24)
- This repo's workspace syncs `.claude/worktrees/*/todo.txt` (whole backlog copies from agent
  worktrees) as documents of the repo: the mirror on B reached 163 documents, B's own clone 213.
- Those copies carry the same `id:` tags as the real backlog, the likely trigger for
  ref:sync-poison-op.

## Design notes
- `txtodo_workspace_paths::workspace_root_from` already stops at a `.git` boundary (a dir or a file,
  for linked worktrees). The walker (`crates/txtodo-daemon/src/walker.rs`) should skip any
  sub-directory holding `.git` the same way.
- Existing workspaces that already synced those files keep their ops; decide whether to tombstone
  them or leave them.
