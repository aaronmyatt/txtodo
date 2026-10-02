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

## As built (2026-10-02, daemon side)
- Order changed: the import/export path went first (line 2), since it was the only reader of the
  mirror; then the upkeep (line 1), then the modules (line 3). Ten commits, each under the 300-line
  diff budget and each compiling: ab4b9f62, ccc74956, f5c8e7a2 (import/export), edbf2556 (mirror
  tests), a6187cdb (sync, open, snapshot, migrate stop using it), ac970434 (field and upkeep),
  8c938048, 0cb18cd3, a7b70977 (modules), c7a94910 (lab tripwire).
- Also gone: `Parked::without_waiting` (fed the mirror only), `CommitTail::{flush, persist_mirror}`
  (`CommitTail` now derives `Default`). `loro_peer` moved to `notes_mirror.rs`. `actor_mirror.rs`
  keeps only the commit extras (fingerprints, flags, broadcast); its name is stale.
- The store still gets `CommitExtras { mirror: None }` until the store line removes the field.
- Not done here: the `txtodo-crdt` list code and the store's mirror row need their own crate lease,
  and the fence needs a clean tree to hand one over; another session's untracked
  `docs/adr/0037-freeze-opkind.md` keeps it dirty. Conflict review A spans proto, CLI, TUI,
  desktop and store.
- Idle RSS after the removal (tests/e2e/idle_rss.rs, 10k lines, 2026-10-02): 1 629 MB. Before: the
  test's documented ~1.7 GB (not re-measured today). So the mirror was not the cause; the RSS gap
  is elsewhere (adoption or the state).
