# editor-save-lost

## Goal

A save from an editor is never overwritten by the daemon's next write.

## Evidence (2026-09-30, found by the p2p lab)

One device, no sync, the lab image (`txtodo-lab:src-cbee7b752392`, 0.0.19 code):

1. `txtodo add "cli-2 +lab"`
2. an editor-style save: copy `todo.txt`, append `editor-2`, write a temp file, rename it over
   `todo.txt`
3. `txtodo add "cli-3 +lab"` straight after

The file ends with `cli-1, editor-1, cli-2, cli-3`: `editor-2` is gone. The daemon logged no
`external_change` reconcile for it; its next `projection_written` wrote over it. A save with a
second or two of quiet around it (`editor-1`) survives.

In `lan-converge` (seed 42, both attempts) every token the no-loss check found missing came from
an editor-style save, none from a CLI edit. There the next write was an incoming sync op.

The lab's `lan-converge` reproduces it (`scripts/lab/lab.sh run lan-converge 42`).

## Cause

- The watcher reports a save only after a 150 ms quiet period (`debounce.rs`), so the actor
  learns of it up to 150 ms late.
- Any commit in that window (a CLI edit, a peer's ops) renders the actor's state, which lacks
  the save, and `commit_inner` writes it straight over the file. It never looks at the disk.
- When the watcher's event does arrive, the file's hash is the daemon's own write, so the
  reconcile is skipped (`ignored_own_write`). The save is gone without a trace.
- `notes.md` is worse: nothing watches it (`notes_actor.rs` module doc). A save is noticed only
  when the notes actor next opens, and any write before then replaces it.

## Design

The design doc already says it (§4.3 step 3): the state "may already be ahead" of the bytes last
written, and the reconcile is three-way. The code assumed state == disk.

- `todo.txt` (`FileActor`):
  - Before writing, the commit checks the disk: our last write (or a recent one), or bytes this
    commit is merging, go ahead. Anything else is someone's save we have not merged: the commit
    lands in the store and the state as usual, but the file is not written. The actor keeps the
    bytes it last wrote (and their task ids): the base of a three-way merge.
  - When the watcher's event comes, the merge runs: reconcile base → disk (the editor's changes),
    stamp those ops, apply them leniently on the current state (a peer's or the CLI's ops
    already in it), commit, write once.
  - Safety net for a missed event: after any message, a pending merge whose file has not changed
    for a second runs anyway.
  - Why not fold the disk in before every write? That reads the file without the debounce, so a
    save written in place (truncate, then write) could be read half-written and turned into
    deletes.
- `notes.md` (`NotesActor`): no watcher, so before `edit`, `import_ops` or `import_updates` the
  disk is compared with the projection, and foreign text is committed first as an `External`
  `NotesEdit`. It can still be read mid-write; watching notes.md is its own task.

## As built (2026-09-30)

- `pending_save.rs`: `may_write` (the gate `commit_inner` asks before the state moves on; the
  first refusal keeps the base, logs `write_held_for_unmerged_save`), `merge_pending_save` (the
  three-way merge; an edit that no longer fits logs `save_op_skipped`), `merge_settled_save`
  (after every mailbox message, for a save quiet for 1 s).
- `external.rs::on_external_change` runs the merge when a save is pending, and marks the bytes
  it reconciles (`absorbing`) so its own write is let through.
- `notes_actor.rs`: `absorb_disk` before `edit`, `import_ops` and `import_updates`; `open` uses
  the same check.
- `replace.rs` already refused a `Replace` while the disk was not our write ("an editor save the
  watcher has not delivered yet"); it now also refuses while a save is held, and the client
  retries.
- Tests: `pending_save_tests.rs` (a save then a CLI add, then peer ops, then undone, then never
  reported) and `notes_actor_sync_tests.rs` (a notes.md save then a peer op). All five fail on
  the old code: each checks the save is still on disk after the daemon's next write.

## Known gaps

- The check and the rename are not atomic: a save landing in the microseconds between them is
  still overwritten.
- `notes.md` saves are picked up at the next write or open, not live; watching notes.md is its
  own task.
- A line the editor changed that a peer changed or deleted in the same window: the editor's op
  does not fit and is skipped (`sync_op_skipped`), the peer's version stays.
