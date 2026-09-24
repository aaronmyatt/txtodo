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

## What the log says (2026-09-25)
- The walker already skips `.claude/worktrees` (`walker.rs::is_skipped_dir`, commit 4f7e1f8f,
  2026-09-20, in every release since v0.0.3).
- This Mac's op log still holds 54,627 ops on 3,697 files under `.claude/worktrees/` (58% of its
  94,385 ops), all stamped 2026-09-13 to 2026-09-19, before that fix. Nothing new since.
- A peer that mirrors the workspace replays all of them and writes those files. That is where B's
  worktree copies came from, not a live walk.

## Design (2026-09-25)
- Walker: skip any sub-directory holding a `.git` entry (a dir, or a file for a linked worktree
  or submodule), the same boundary `workspace_root_from` uses. The watcher reuses
  `is_in_skipped_dir`, so it follows.
- Sync import: an op on a path under `.git`, `node_modules` or `.claude/worktrees` goes into the
  log only (`Store::append_with_source`, source "sync"). No file, no actor. Heads stay dense and
  the junk is never written on the peer. `target/` is left out: a ref dir can be called that.
- The old ops stay in the log. Heads are op counts, so dropping them would break every peer that
  already holds them. Leave them; the import rule above keeps them off disk.

## Known gaps
- A peer that already wrote the copies (B) keeps them until someone deletes them by hand.
- The first mirror of this workspace still moves the 54k junk ops over the link.

## As built (2026-09-25)
- `walker::is_skipped_dir` also skips a directory holding a `.git` entry (dir or file). The
  watcher goes through `is_in_skipped_dir`, so it follows. The root itself is never checked.
- `walker::is_skipped_path` (names only: `.git`, `node_modules`, `.claude/worktrees`) gates
  `lan_apply::commit_one_file`: such ops go to the log with source "sync" and no file or actor.
- Tests: walker (nested clone and linked worktree skipped; the path check), and
  `lan_session_resend_tests::ops_on_a_worktree_copy_land_in_the_log_but_never_on_disk`.
- Full daemon suite: 543/545 in one run; the two misses were `relay_multiplex` and
  `relay_converge`, which dial the public n0 relay and time out under suite load. Both pass alone.
