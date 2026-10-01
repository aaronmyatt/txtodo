# partition-converge

## Goal

Two devices that keep editing while apart end with the same files once they reconnect, whatever
order the ops arrive in.

## Evidence (2026-10-01, the p2p lab)

`lan-converge` fails on every seed at "partition healed": b1 leaves the LAN, both devices edit,
b1 comes back, and the files still differ 2 min later. Seed 830835642, report
`~/.local/state/txtodo-lab/reports/20261001-004825-lan-converge/`:

- **Delivery is fine.** Both devices' op logs (`default/.txtodo/oplog.db` inside each
  `devices/<dev>/workspace.tar`) hold the same ops: 27 from a1 and 41 from b1 in `todo.txt`, and
  the same counts in every other file. Query: `SELECT file, hex(device), count(*) FROM ops GROUP
  BY file, device`.
- **Application is not.** Same lines on both sides, but the run of nine lines b1 added while
  apart sits mid-file on a1 and at the end on b1 (`diff-final.txt`). The same ops, applied in
  another order, render another file.
- The lab's random workload mixes in other bugs; `concurrent-adds` (adds and `do` only) passes
  since insert-order, so the gap is in what that scenario leaves out: moves and deletes racing
  inserts.

## Causes

1. **An anchor names a task, not a spot** (known gap in `tasks/insert-order/notes.md`).
   `Insert { after: T }` lands after T wherever T is when the op arrives. If another device
   moved T in the meantime (`do` moves a done line to the bottom, archive, reorders), the add
   lands at T's old spot on one device and its new spot on the other. If T was deleted, the add
   is skipped (`sync_op_skipped`, no task): deletes remove the entry, there is no tombstone. b1's
   partition run was anchored on b1's last line, which a1 had `do`-moved.
2. **Two edits to one line apply in arrival order** (likely; not in this report's diff, seen in
   earlier runs as two texts of one line). `fields.rs::set_field` and `edit_text` overwrite
   whatever is there; nothing compares stamps. The Loro mirror does (`txtodo-crdt` `lww.rs`,
   `write_if_newer`), but the mirror never decides bytes.

## Design options for cause 1 (decide first; ADR either way)

- **A. Ghost placements, no wire change.** A deleted task, and the spot a moved task left, stay
  in `DocState` as hidden entries (not rendered). An op anchored on T resolves to T's placement
  with the newest stamp not newer than the op's own; older placements are ghosts other ops can
  still sit after. Every device resolves the same way from the same ops, so it converges.
  Costs: ghosts grow with history (rebuilt from the log at open, like the stamps; compaction
  needs every peer's acks), and "newest placement not newer than the op" is a stamp rule, not
  what the author saw, so a concurrent add can follow a moved line. This is Kleppmann et al.
  2020, "Moving elements in list CRDTs", with the move's old spot as a tombstone.
- **B. Anchors carry the anchor's placement op id.** New `OpKind` variants (append-only enum,
  signing bytes change), a protocol version bump; an old peer cannot decode them. Exact intent,
  bigger change.
- I'd take A: it needs no wire change, and its gaps (ghost growth, stamp rule) are bounded.

For cause 2: field ops last-writer-wins by stamp in `DocState`, per field, as the mirror already
does. Description text: last-writer-wins per task for now (char-level merge is the mirror's job;
see design §4.2), so two edits never render two texts.

## How to check

- Unit: a `state_order_tests.rs`-style permutation test over (add after T, move T), (add after T,
  delete T), (two edits of one line). Every arrival order must render the same bytes; today
  each fails.
- Lab: a scenario like `concurrent-adds` that also moves (`do`, archive) and deletes across a
  partition; then `lan-converge` on three seeds, and `chaos`.
- Op-log comparison (above) first, on any failing report: same ops on both devices means an
  application bug, different counts a delivery bug.
