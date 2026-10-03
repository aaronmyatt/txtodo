# sync-batch-txn

Deferred 2026-10-03 (human call). Option A from `tasks/first-sync-speed/notes.md`
("One commit per batch"); option B, coalesced file writes (`write_defer.rs`), went first.

## Goal

A received peer batch lands as one store transaction across every file it touches, not one
commit per same-file run. Fewer SQLite commits and one projection write per file, whatever the
order of files inside the batch.

## Why not now

- B already takes most of the cost: the file write and fsync per run were ~40% of a sync commit,
  and B writes each file once per batch (bench, 20 rounds, debug: session 11.7 → 3.4 s).
- What is left per run is the store commit, the `DocState` clone and the render. A needs more
  than a store API.

## Why it is not just "group the batch by file"

- A batch interleaves files inside one origin's run (X, Y, X). Grouping by file inserts that
  origin's ops out of order. The store takes a peer's numbers as given (ADR 0039), so numbering
  survives, but heads are `MAX(origin_seq)`.
- If X's commit then fails (disk, or the `blocked/todo.txt` mkdir case in
  `lan_session_resend_tests`), Y's later ops have already landed past X's missing ones. The ack
  and the heads skip them, and no peer sends them again: lost, where today the batch stops at X
  and is retried (stuck, not lost).

## Design sketch

- `txtodo-store`: a multi-file commit, e.g. `commit_batch(&[(file, ops, projection, prev_hash,
  extras)])` in one transaction, ops inserted in batch order so origin numbers stay dense.
- Daemon: a prepare/commit turn between `FileActor`s. Each actor applies its runs to a clone of
  its state and hands back the projection and extras (prepare), without committing. One caller
  commits them all, then tells each actor to swap in its state and write (commit), or to drop
  it (abort).
- Mkdir and register for new files happen before prepare, so a failure there stops the batch at
  that file's first op, and the prefix before it commits as one transaction.
- Parking (`sync_park.rs`) and the op-set hash fold in at the actor's commit turn, as now.

## Open questions

- This changes the one-writer-per-file commit path (`actor.rs`'s `commit_inner`): an actor's
  state then moves on a turn it did not start. Needs an ADR.
- Lock order: the store mutex is held across the batch transaction while several actors wait in
  their prepare turn. Deadlock risk with an actor that locks the store on its own turn.
- Is it still worth it after B? Measured 2026-10-03 (macOS `sample`, debug): the per-run store
  commit is ~15–25% of `on_sync_run`, the file writes ~45% (already one per file per batch). So
  A saves at most about a fifth of a sync commit.
