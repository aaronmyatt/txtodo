# txtodo-crdt

## Purpose
The Loro merge engine (plan M4, ADR 0002/0013): one `LoroDocument` per workspace with a movable
list per file and a shared `tasks` map; our `Op`s in, Loro updates between devices, `Op`s and
review flags out. As built 2026-09-12.

## Public interface
- `LoroDocument::{open, hydrate(ops), fork, at(frontiers), snapshot, from_snapshot, set_peer,
  version, export_updates(since), import(bytes) -> Imported}`; views `list_ids(file)`,
  `is_deleted`, `description`, `set_description`, `last_blank_id`; `is_blank(id)`,
  `rebuild_line(doc, task)` (canonical, not byte-faithful).
- `apply(doc, &Op)` — `OpKind → Loro`, one commit per op, exhaustive; Insert of a deleted id
  resurrects it; `BlankRemove` skips tombstones. `hydrate_file(doc, file, lines, hlc)` — O(n) bulk
  load of an empty list. `from_batch(doc, diff, stamp, mint)` — Loro diff → `Op`s.
- `Lww { value, hlc }`, `write_if_newer` (ADR 0013: our HLC arbitrates; an equal stamp lands).
- `detect(doc, &Imported) -> Review { flags: Vec<ReviewFlag { task, file, mine, theirs }>,
  overflowed }`, `MAX_REVIEW_FLAGS_PER_FILE`.
- `NotesDoc` (plan M5, tasks/crdt-notes-doc): a single root `LoroText`, no tasks map, no movable
  list — deliberately not a `LoroDocument`, since notes are prose, not task lines. `open`,
  `hydrate(initial)`, `from_snapshot`, `set_peer`, `content`, `apply_edits` (replays a dual-indexed
  `TextEdit` stream through `to_loro::replay_edits`, the same walk a description edit uses),
  `snapshot`, `version_bytes`, `export_updates_since`, `import`. Concurrent edits from two devices
  merge character-wise through Loro's text CRDT; `txtodo-daemon` derives its own op-log entry from
  the text before/after an import rather than from a Loro diff (no task list to reconcile).

## Invariants
- Untouched lines materialise byte-identical — the host keeps the bytes (`txtodo-daemon`
  `DocState`); this crate holds fields + text and never claims to re-emit quirks.
- One id, one list entry: a deleted task is a tombstone (`deleted` register), never removed.
- Every list mutation goes through the per-file shadow (`doc/shadow.rs`); an import or snapshot
  load invalidates it. Concurrency is asked of Loro frontiers, never of the `Hlc`.
- Two devices merge only if their documents share lineage (fork/snapshot at pairing, then
  updates); replaying our `Op`s into independent documents does not converge.
- May depend only on: txtodo-model, txtodo-store, txtodo-core.
