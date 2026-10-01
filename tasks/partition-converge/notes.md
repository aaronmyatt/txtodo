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

## EditText cannot win by stamp alone (found 2026-10-01)

The plan above says description text is last-writer-wins per task. That works for `SetField`,
which carries a whole value: skip an op older than the field's stamp, and every order ends on the
newest value. It does not work for `EditText`: its edits are splices (`TextEdit::Insert/Delete`
at char offsets) on the text its author saw. A HLC cannot tell "B edited after seeing A" (apply B
on A's result) from "B edited at the same time as A" (B's offsets are against the old text). So
skipping the older op gives `base+B` on one device and `base+A+B` on the other.
`state_converge_tests::two_devices_editing_one_description_agree` shows it (ignored).

Options (an `@human` call, the "Decide: EditText convergence" line):
- **T1. Text history per task in `DocState`, replayed in stamp order.** Keep each task's text ops
  since its last whole-text point; on a late arrival, rebuild the description by applying them in
  HLC order, skipping ones that no longer fit. No wire change. Costs: memory per edited task
  (bounded like ghosts: rebuilt at open, trimmed only with every peer's acks), and a splice
  replayed on a different base can still land oddly (it applies, just not where its author meant).
- **T2. `EditText` names its base** (the text's hash, or the op id it was made on). A wire change
  (new variant, protocol bump). Exact: a peer can tell causal from concurrent.
- **T3. Let the mirror decide description bytes** (Loro text already merges by character). Breaks
  "the mirror never decides bytes" (an ADR-level change).

I'd take T1 if the placement decision goes with A (same kind of state: kept history, rebuilt at
open), T2 if it goes with B (one protocol bump for both).

**notes.md has the same problem, and it is now the lab's main failure** (run 20261001-155749,
all 10 scenarios on 2026-10-01 code, seed 1072683562). A peer's `NotesEdit` is a splice too, and
`NotesActor::import_ops` applies it in arrival order (the Loro mirror gets the same splices, so it
does not merge either). In 6 of the 8 failing scenarios (lan-to-relay, nat-holepunch, sleep,
old-new, bad-link, clock-skew) the only file still different at the end is `tasks/lab/notes.md`:
the same appended lines in another order, and in some a line lost. lan-converge's `todo.txt` also
ends with one line in two texts (`+lab @a pri:B1` vs `+lab @a1 pri:B`): a splice landed at
another offset. relay-only passes. So the EditText decision should cover `NotesEdit` too; T3 is
more natural there, since notes already are a Loro text document.

## How to check

- Unit: a `state_order_tests.rs`-style permutation test over (add after T, move T), (add after T,
  delete T), (two edits of one line). Every arrival order must render the same bytes; today
  each fails.
- Lab: a scenario like `concurrent-adds` that also moves (`do`, archive) and deletes across a
  partition; then `lan-converge` on three seeds, and `chaos`.
- Op-log comparison (above) first, on any failing report: same ops on both devices means an
  application bug, different counts a delivery bug.

## As built

- 2026-10-01, the permutation test: `state_converge_tests.rs` runs every arrival order of two devices' ops,
  skipping an op that does not apply as sync does. Two anchor cases and the two-edits case stay
  ignored until their decisions (the two `Decide:` lines).
- 2026-10-01, stamp-wins fields (`SetField` half): `DocState.field_stamps` keeps each task field's
  newest `SetField` stamp; an older one is a no-op (`set_field_older_than_field`, debug). Taken
  from the replay at open (`adopt_stamps`), settled with a commit like line stamps, dropped when
  the task is deleted or moves to another file. Bounded by the tasks in the document. The
  `EditText` half waits on the EditText decision.
- 2026-10-01, ghosts (option A, ADR 0033): `state_ghosts.rs` keeps `hidden`/`visible` beside
  `entries`/`stamps`. Delete, blank remove and move-away hide in place; a stale move and a move of
  a deleted task add their spot as a ghost; anchors resolve to the newest placement not newer than
  the op, the shown one on a tie (one commit, or a hydration replay that stamps every op alike: two
  reconcile-replay tests caught the tie). Rebuilt at open by `adopt_stamps` taking the replay's
  whole sequence when it shows the same lines by id (a tag-stripping migration included); bounded
  at 10 000 per document, oldest first. Deleting again, or moving a task that is gone, is now a
  no-op instead of an error. Tests: the two anchor cases un-ignored, plus a move that lost to a
  newer one, a blank removed past a deleted line, ghosts back at open, the bound. Known gaps: an
  older placement of an anchor arriving after an op anchored on it (the stamp rule's limit, in
  the ADR); history replays from a snapshot start with no ghosts.
