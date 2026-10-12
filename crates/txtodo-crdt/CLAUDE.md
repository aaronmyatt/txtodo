# txtodo-crdt

## Purpose
The `notes.md` merge engine (plan M5, ADR 0002): `NotesDoc`, one Loro text per notes file. As
built 2026-09-12 with a todo.txt list document beside it; that document, its op translation
(`to_loro`/`from_loro`), the LWW registers, `resurrect` and the review flags went with the
daemon's todo.txt mirror (ADR 0038, 2026-10-13). `DocState` in `txtodo-daemon` is the todo.txt
engine.

## Public interface
- `NotesDoc` (tasks/crdt-notes-doc): a single root `LoroText`, no tasks map, no movable list.
  `open`, `hydrate(initial)`, `from_snapshot`, `set_peer`, `content`, `apply_edits` (replays a
  dual-indexed `TextEdit` stream through `replay_edits`), `snapshot`, `version_bytes`,
  `export_updates_since`, `import`. Concurrent edits from two devices merge character-wise
  through Loro's text CRDT; `txtodo-daemon` derives its own op-log entry from the text
  before/after an import (`notes_mirror.rs`).
- `NotesDocError`.

## Tests
- Unit tests in `notes.rs`: hydrate and edit, snapshot round trip, two devices' concurrent edits
  merging. The conflict table (`specs/conflicts.md`) runs on `DocState` in
  `txtodo-daemon/src/conflict_table_tests.rs`.

## Invariants
- `replay_edits` translates the dual-indexed stream (a `Delete`'s `at` counts the source, an
  `Insert`'s the target) through two cursors; positions are Unicode code points.
- Two devices merge only if their documents share lineage (fork/snapshot, then updates).
- Logs carry ids, counts and hashes — never line text, tokens or payloads.
- May depend only on: txtodo-model, txtodo-store, txtodo-core. Uses txtodo-model and
  txtodo-core; the txtodo-store dependency is unused since ADR 0038 (dropping it rewrites the
  frozen Cargo.lock).
