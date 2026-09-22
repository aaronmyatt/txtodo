# notes-sync

## Goal

`notes.md` (root and every nested `tasks/<slug>/notes.md`) syncs across paired devices the same
way `todo.txt` already does. Today it does not: this is a real, documented gap, not a hypothetical.

## Why

Found while answering a user question about cross-device sync scope. `Workspace::register`
(`crates/txtodo-daemon/src/workspace.rs:166-172`) refuses to build a `FileActor` for any notes
document (`walker::is_notes_document` → `Ok(false)`, no actor). Consequences:
- No recovery: an on-disk `notes.md` is not seeded into an actor at daemon startup, unlike
  `todo.txt` (`FileActor::recover`).
- Incoming LAN sync ops addressed to a notes path hit `lan_apply.rs`'s `get_or_create_actor`,
  which calls `register()` → `Ok(false)` → no actor found → the op is silently dropped.
- `crates/txtodo-daemon/tests/nested_ref_sync.rs` deliberately excludes `child/notes.md` from its
  convergence assertion because it does not converge.
- The design doc already says otherwise: `txtodo-design.md:155` ("Every `todo.txt` and
  `notes.md`... at any depth, is a synced document") and `docs/questions.md:9`. Implementation
  hasn't caught up.

## Design (open — not yet decided)

- Does `notes.md` get its own `FileActor`-equivalent (byte-range CRDT like `todo.txt`), or does it
  stay a simpler whole-document type given `EditNotes` already replaces the full text? Whichever
  shape, it needs the same recover-on-startup + LAN-apply-target behaviour `todo.txt` has today.
- `NotesActor`/`NotesRegistry` already exist and work for local `GetNotes`/`EditNotes` — the gap is
  wiring them into `Workspace::register`'s discovery/recovery/LAN-apply paths, not building new
  CRDT machinery from scratch.

## Open questions

- Concurrent-edit conflict story for notes.md: does it get the same `needs_review` line-level
  flag `todo.txt` uses, or does whole-document `EditNotes` need its own conflict model (two
  concurrent full-text edits don't line up the way two line edits do)?

## As built (2026-09-23)

- Decided: no `FileActor` for notes; the existing `NotesActor` gained the two missing behaviours.
  `open` reconciles disk against the stored projection (or, right after pairing, the shipped
  mirror's text) and commits the difference as one `NotesEdit` op from `Principal::External`, so
  a hand-written notes.md has something for a peer's `Want`. `import_ops` applies a peer's
  `NotesEdit` batch like `FileActor::on_sync_ops` (state, store, disk, mirror; all or nothing).
- `Workspace::register_discovered` opens every discovered notes.md so the seed happens at daemon
  start; a notes file that will not open is logged, never fatal to the workspace.
- `lan_apply::commit_notes_file` is the receiving side; the directory is made first.
- Proven by `tests/nested_ref_sync.rs` (root and nested notes.md converge over real LAN).
- Open: the conflict story. Two concurrent whole-document edits merge char-wise through the
  mirror on the `import_updates` path, but the LAN op path applies edits in arrival order with
  no `needs_review` flag; a hand edit on disk while the daemon runs is still not watched (no
  notes watcher, as before).
