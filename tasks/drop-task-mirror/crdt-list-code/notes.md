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

## As built
- conflict table (2026-10-13): 90188835, `crates/txtodo-daemon/src/conflict_table_tests.rs`. Rows
  1-5, 8, 10 pass on `DocState` in every arrival order. Rows 6-7 fail there even with the edit or
  complete stamped after the delete (checked with `--include-ignored`), so they are ignored with
  the open question as the reason. The spec now points at this file.
- list modules (2026-10-13): f36f5079..69525279. Tests first (lww, remove_tag, view, no_secrets,
  roundtrip), then hydrate, resurrect, doc/sync, from_loro (+ rebuild_line), to_loro, the doc
  module, lww. `from_loro` (609 lines with its cuts), `to_loro` (426) and `doc` (405) went one
  module per commit, over the 300 budget: each is one file over 300 or a dead module whose
  piecemeal pruning left only a shell; the messages say so. What is left: `lib.rs` and
  `notes.rs` (`NotesDoc`, `replay_edits`). Unused now: `txtodo-store`, `tracing`, dev
  `tracing-subscriber` (removing them rewrites the frozen Cargo.lock: a human line).
