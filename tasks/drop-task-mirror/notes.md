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

## As built (2026-10-11, the rest)
- Store: a todo.txt commit lands no mirror row (`CommitExtras::mirror` gone). `put_mirror` /
  `get_mirror` stay: notes.md's `NotesDoc` keeps its snapshot in the same table (found when the
  daemon build broke on `notes_actor.rs`); they moved to `mirror.rs`.
- Conflict review A: its own sub-backlog, `drop-review-flags/` (desktop, TUI, CLI, MCP, daemon,
  store, crdt). Each client now shows duplicate groups only; `ResolveConflict` refuses with
  `failed_precondition`; MCP's `todo_conflicts_resolve` is gone and `todo_conflicts_list` lists
  duplicate groups (a tool-surface change agents see).
- crdt: only `NotesDoc` and `text::replay_edits` are left; the sim and `just sim` went too.
- Left, a human call: deleting the flag messages and `ResolveConflict` from the proto.

## From the other device's build (restored 2026-10-26)
The 2026-10-26 merge kept the remote side of this file; these lines are the local side's. Their
commit hashes are on the local side (`85756893^1`); HEAD runs the remote side's equivalents.

### As built (2026-10-12, store line)
- `CommitExtras::mirror` and its write in `land_extras` are gone (9ad4e198), with `last_seq_on`,
  which only it used. The `mirrors` table, `put_mirror`/`get_mirror` and `MAX_MIRROR_BYTES` stay:
  `notes_actor.rs` keeps the notes.md mirror there. Old todo.txt rows stay in the table, unread.
- Three commits, because the fence leases one crate at a time and the field spans two: daemon
  stops naming the field (d96b5b76), store drops it, daemon goes back to a plain literal.
- Known gap: `cargo clippy --workspace` fails on `txtodo-cli` with clippy 1.99's new
  `chunks_exact_to_as_chunks` lint (`commands/edit.rs:134`). It fails on HEAD before this too; not
  touched here. `cargo nextest` is not installed on this Mac, so the fast gate's test step fails.
