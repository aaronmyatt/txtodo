# Drop the Loro mirror of todo.txt documents

## Goal
Remove the per-`todo.txt` Loro mirror and the dead import path on it. Nothing in production reads
it, and it logs mirror errors in every lab chaos and clock-skew run. Design and evidence:
`docs/adr/0038-drop-the-todo-txt-loro-mirror.md` (proposed 2026-10-02).

## Design
- Order: decide first (ADR 0038 plus the conflict-review question), then the daemon removal, then
  `txtodo-crdt`, store, lab and docs. Each line below is one commit.
- Keep the `notes.md` mirror (`NotesDoc`): it is read, by the held-save merge (`notes_held.rs`).
- No wire change, no protocol bump.
- The check that nothing reads it (2026-10-02): grep over Rust, TS and Svelte in the checkout,
  every worktree and git history; `import_updates`/`export_since`/`version` only in
  `import_tests.rs`. Not covered: a client reading the store's mirror row from SQLite directly.

## Decided 2026-10-02 (owner)
- ADR 0038 accepted. Conflict review A: the `needs_review` flags and `Resolve`'s Mine/Theirs go
  with the mirror; the conflicts view keeps duplicate groups only.

## Open questions (answered above)
- Same-word conflict review (`needs_review` flags) dies with the import path. A: drop it (today's
  behaviour); B: rebuild it on the state's text history (ADR 0034), its own ADR. See the ADR.
