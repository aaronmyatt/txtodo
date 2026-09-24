# sync-poison-op

## Goal
One change a peer cannot apply must not stop that workspace syncing forever, and the reason must
be findable.

## Evidence (2026-09-24, B's log, workspace `01M2RZ8EX1CQAS21TNZ5YY6PBT`, this repo mirrored)
- `mirror_refused_converging` on `.claude/worktrees/competent-solomon-903154/todo.txt`: "mirror
  refused Insert: no task 01M2B4ZWQE9P391C3B1E380S15 in the list".
- `lan_sync_ops_refused` on `todo.txt`: "apply op: no task 01M2B4ZWQGR07FHF23M0CEQX0X in this
  document". `commit_incoming_ops` fails the whole batch, so heads stay at 17000.
- The next batch (18001..=19000) commits but its `Ack` is refused as a gap; the session sits in
  `Importing`, the next `Ops` is refused, and the link ends. Every reconnect replays from 17000 and
  fails at the same op.

## Open
- Root cause of "no task … in this document". First suspect: `.claude/worktrees/*/todo.txt` are
  copies of the backlog with the same `id:` tags, so one task id lives in many documents
  (ref:walker-nested-checkouts). Second: op order across files (a `Move` split into two ops).
- What to do with an op that cannot apply: skip and flag (needs_review), or park the run and keep
  the rest flowing. Either way the gap rule in `advance` needs a story.
