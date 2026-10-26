# txtodo-crdt

## Purpose
The Loro merge engine for `notes.md` (plan M5, ADR 0002): `NotesDoc`, one root `LoroText` per
notes document. The todo.txt task-list document (`LoroDocument`, op translation, review flags,
LWW registers, the sync simulator) went with ADR 0038 on 2026-10-11: nothing read it.

## Public interface
- `NotesDoc` (tasks/crdt-notes-doc): no tasks map, no movable list — notes are prose. `open`,
  `hydrate(initial)`, `from_snapshot`, `set_peer`, `content`, `apply_edits` (replays a
  dual-indexed `TextEdit` stream through `text::replay_edits`), `snapshot`, `version_bytes`,
  `export_updates_since`, `import`. Concurrent edits from two devices merge character-wise
  through Loro's text CRDT; `txtodo-daemon` derives its own op-log entry from the text
  before/after an import.
- `NotesDocError`.

## Invariants
- In-memory only: `txtodo-store` is the durable truth; a `NotesDoc` is rebuilt from a snapshot
  plus replayed ops.
- Two devices merge only if their documents share lineage (snapshot at pairing, then updates).
- Logs carry ids, counts and hashes — never note text.
- May depend only on: txtodo-model, txtodo-core.
