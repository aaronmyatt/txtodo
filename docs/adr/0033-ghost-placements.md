# 0033 — Keep deleted and moved-away placements as hidden ghost entries

- Status: accepted 2026-10-01 (the owner chose option A in `tasks/partition-converge/notes.md`)
- Date: 2026-10-01
- Deciders: project owner

## Context
Devices that edit while apart do not converge once they reconnect (p2p lab, every scenario with
a partition or concurrent edits). The ops all arrive; applying them gives different files. The
main cause: an op's anchor names a task, not a spot (`Insert { after: T }`). If another device
deleted T meanwhile, the insert is skipped (`sync_op_skipped`) and the line is lost on that
device, and every line anchored on it after that. If another device moved T (`do` moves a done
line to the bottom), the insert lands at T's old spot on one device and its new spot on the
other. The 2026-10-01 lab run lost 5–7 lines per scenario this way.

## Decision
We will keep every placement a document ever had in `DocState`'s sequence:

- Deleting a task, removing a blank, and moving a task away (same file or to another) hide the
  entry where it is, with its stamp, instead of removing it. A hidden entry is a ghost: never
  rendered, never counted in a line number, never returned by any by-position call.
- An op anchored on task T resolves to T's placement with the newest stamp not newer than the
  op's own (live or ghost), then RGA's skip rule places it as before (`state_order.rs`).
- A same-file move older than T's live placement (it lost to a newer move) and a move of a task
  that is already deleted add their spot as a ghost, so every device ends with the same
  placements whatever order the ops came in.
- Ghosts are rebuilt from the log at open (the replay from empty that log repair already runs),
  and at most `MAX_GHOSTS_PER_FILE` are kept; the oldest go first.

No wire change and no store change: ghosts are derived from the ops every device already holds.

## Consequences
- Good: an add after a line another device deleted is kept everywhere; an add after a line
  another device moved lands in one place.
- Bad: memory grows with deletes and moves, up to the bound. A ghost dropped by the bound
  makes an op anchored on it skip again, as before this ADR.
- Bad: "newest placement not newer than the op" is a stamp rule, not what the author saw: a
  concurrent add can follow a line another device moved. Option B (anchors that carry the
  anchor's placement op id) gives the author's intent, at the cost of a wire change.
- Amended 2026-10-01 (lab lan-converge seed 435090918): an older placement of the anchor (a
  concurrent move) can arrive after an op anchored on it, and the op then sat under the anchor's
  previous placement on that device only. Each slot now keeps its parent (the placement its op
  followed); a placement that lands late takes, with what follows them, the entries that should
  follow it (`state_rehome.rs`). A parent dropped by the ghost bound ends that block early.
- Amended the same day, same run: which blank a `BlankRemove` hides depended on what had landed
  when it did, so two devices hid two different blanks. A `BlankRemove` is now kept as an eraser,
  a hidden slot placed and re-homed like any other; after each op every eraser, in sequence
  order, claims the first unclaimed blank after it that its author could have seen
  (`state_erase.rs`). One that finds none is kept and may claim a blank that lands later; a
  delete between an eraser and a blank can let it claim that blank. The stamp rule still applies:
  a remove can follow its anchor to a spot a concurrent move gave it and take the blank there.
- Neutral: history replays that start from a snapshot (checkout, undo) start with no ghosts from
  before it; snapshots hold only the bytes.

## Alternatives considered
- B. Anchors carry the anchor's placement op id: exact intent, but new `OpKind` variants, new
  signing bytes and a protocol bump; an older peer cannot decode them.
- Tombstones only for deletes, no move ghosts: fixes the lost lines but not the moved-anchor case.
