# todo-log-repair

## Goal

After notes-no-base shipped, 103 of the 238 `.txt` lists in the other Mac's copy of this Mac's
`txtodo` workspace still differed from this Mac. Checked on 2026-09-28 by replaying this Mac's
real op log the way a fresh peer does (scratch simulation, not committed):

- 79 lists: this Mac's own log does not rebuild its own file. Old reconciles (2026-09-24/25)
  turned `txtodo do` into `Completed` + `CompletionDate` ops that replay to `x D desc`, while the
  file has `x D D desc` (creation date lost). This Mac noticed (`exact: false`), adopted the file
  and pinned it with a snapshot. Snapshots never sync, so a peer keeps the lossy text. The other
  Mac matched the replay hash for hash.
- 24 lists: this Mac had 184 duplicated lines. A burst of inserts from the other Mac's device id
  (2026-09-25 18:02 +0800) re-added lines from an older checkout; the other Mac never applied
  them. Removed here by task id (161 copies, 22 lists; 2 lists hold older duplicates of their own
  and were left alone).
- Retrying skipped ops (option B in the chat) fixed 0 of the 103 in every order simulated, so it
  was dropped.

## Design

Same idea as notes-no-base, for task documents. After `FileActor::recover`, replay the file's
log from empty, ignoring snapshots (what a peer has), skipping what does not fit the way
`on_sync_ops` does. If that renders other bytes than the projection, commit
`reconcile_replay::replayable_ops(replayed, current state)`: ops by task id that take the
replayed state to this one, each checked on a scratch copy. The commit keeps the projection and
the file as they are and forces a snapshot at its seq, so `history::replay` starts from the file
and never applies the repair on top of the older snapshot.

Ops are by task id, not position, so a peer whose replay differs a little still takes most of
them; any that do not fit are skipped there (logged).

## Known gaps

- A peer whose own copy diverged for another reason (order of multi-device ops) is not detected
  by the origin: the origin's log rebuilds the origin's file, so it sees nothing to repair.
- 2026-09-28: the dedupe went through `txtodo mcp --stdio`, which started a second `txtodod --dir`
  on the same store. Each delete landed twice in the log (device 01A0B561 via MCP, then this
  Mac's reconcile of the "external" write), so a replay has two blank lines per delete. The
  repair here fixes that on the next open. Spawn bug flagged as its own task.
