# crdt: drop the todo.txt list code

## Goal
ADR 0038: `txtodo-crdt` keeps what `NotesDoc` needs and drops the list code nothing else calls
(`LoroDocument`, `doc/`, `from_loro`, `hydrate`, `lww`, `resurrect`, `to_loro`). About 3 400
lines with their tests, so several commits under the 300-line budget.

## Design
- `NotesDoc` uses one thing from the list code: `to_loro::replay_edits`. It moves into
  `notes.rs` first, so the rest can go without touching notes.
- `tests/it/conflicts.rs` is the executable form of the normative `specs/conflicts.md`
  (`row_count_matches_this_table`), but against the Loro list engine, which production no longer
  runs. The rows move first to `txtodo-daemon` as `conflict_table_tests.rs` on `DocState`, every
  arrival order (the `state_converge_tests` helpers), one test per row in row order.
- `tests/it/sim.rs` (Loro convergence simulator) goes with the engine. `just sim` then runs no
  test; the recipe is in the frozen `justfile`, so its removal is a human line.
- Unused deps on the crdt crate (`txtodo-store`, maybe `txtodo-core`) would rewrite the frozen
  `Cargo.lock`: a human line too, and harmless left in.

## Open question (@human)
- Rows 6-7 of the spec: "delete | edit → edit wins, task resurrected" and "delete | complete →
  completed, not deleted". The Loro list engine did that (`crate::resurrect`). `DocState` keeps
  the task deleted: an edit of a deleted task applies hidden
  (`state_converge_tests::an_edit_of_a_line_another_device_deleted_changes_nothing`,
  `state_reinsert.rs`). Production has always run `DocState`, so the spec has been wrong about
  production. Until decided, the ported rows 6-7 assert the spec and are `#[ignore]`d with this
  reason, like row 9.
  - A: change the spec to "delete wins" (today's behaviour). Small; the docs say what ships.
  - B: change the engine to resurrect on a newer edit. Needs its own ADR (ordering, ghosts, sync).
  - I'd take A unless resurrecting is a promise users rely on: it is what every device does today.
