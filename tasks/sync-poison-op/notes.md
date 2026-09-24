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

## Root cause (2026-09-25, from this Mac's op log, `.txtodo/oplog.db`)
- Seq 23778 on `todo.txt` is `Move { task 01M2B4ZWQC1T4A400EQDQ5DSR2, after 01M2B4ZWQGR07FHF23M0CEQX0X }`.
  The `Insert` of `...CEQX0X` into `todo.txt` is seq 23917, later in the same commit (all ops
  stamped 2026-09-18 01:06:28, one HLC). A peer replaying in order hits "no task ... in this
  document" at 23778.
- The same commit inserts task `06G9TNVQFSNMFB1F668PTYEZ2M` twice.
- Why the sender logged it: `external.rs::derive_reconciled_ops` replays the reconciler's ops on
  a clone. When that fails or renders different bytes (`exact: false`), it adopts the file and
  commits the ops anyway. The local snapshot hides the damage; a peer only has the ops.
- It is not rare: `ops_derived` with `exact: false` 54 times on 2026-09-24 and 65 on 09-20 here,
  about one reconcile in eight.
- The worktree copies are not the cause of this one; they are ref:walker-nested-checkouts.

## Design (2026-09-25)
- **Sender.** On `exact: false`, synthesize ops that do replay: delete tasks gone or changed past
  what `change_ops` can express, remove every blank, then walk the target top-down (Move a present
  task after its predecessor, Insert a missing one), then put blanks back after the task above
  them. Replay on a clone; commit them only when they reproduce the target bytes. Else the old
  path, logged at warn.
- **Receiver.** Sync import applies leniently. An op that fails is retried once the rest of its
  same-HLC group has applied (the anchor-after-a-later-insert case). What still fails is skipped:
  the op stays in the log (heads stay dense, other peers still get it) and a `sync_op_skipped`
  warn names the file, op, kind, task and reason.
- The history already in the log cannot be rewritten (heads are counts). A peer that replays it
  converges on everything but the skipped ops.

## Known gaps
- A skipped op is findable in the log only. No `needs_review` flag: that table holds description
  conflicts (mine/theirs) and the task may not exist.
- The receiver's copy can differ from the sender's where an op was skipped, until a later edit
  of those lines.
