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

## As built

- 2026-09-28/29: `80c2ce08` drops the 161 stale copies (by task id through MCP `todo_delete`,
  then the leftover blank lines by hand). `3b0f8099` adds `log_repair.rs` and its tests.
  `b1f5bda7` is release 0.0.16.
- `replay_from_empty` reuses `sync_ops::apply_leniently` (now `pub(crate)`), so the repair's base
  skips exactly what a peer's import skips. It pages through the whole log; a cut-off replay
  makes no repair.
- The repair commit keeps the file and projection, does not feed the mirror (snapshot path
  converges it), and forces a snapshot so `history::replay` starts from the file.
- Checked on a copy of this Mac's real store (throwaway test, not committed): 103 of 236 lists
  repaired at open; the root `todo.txt` alone got 23460 ops (its log replays to ~8976 lines, most
  from ~4300 test-daemon ops written into the real log on 2026-09-23). A fresh peer importing
  each log device by device then rebuilds 234 of 236; the other Mac as modelled, 235 of 236.
  Boot cost on that copy: ~6 s for all 236 opens in a debug build, most of it the one-off root
  repair.
- Still open: `security-m8-review` and `sync-live-push` differ only by multi-device op order; the
  origin's own log rebuilds them, so it sees nothing to repair. Not checked yet on the two real
  Macs.
