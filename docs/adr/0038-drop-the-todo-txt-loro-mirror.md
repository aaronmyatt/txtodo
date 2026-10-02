# 0038 — Drop the Loro mirror of todo.txt documents

- Status: accepted 2026-10-02 (the owner chose conflict review A in `tasks/drop-task-mirror/notes.md`)
- Date: 2026-10-02
- Deciders: project owner

## Context
Each `todo.txt` actor keeps a Loro copy of its document (`mirror.rs`). Every commit feeds it,
checks it against the state, and heals it (`converge_to`) or rebuilds it (a new lineage); its
snapshot is saved in the store. It was built to be the sync engine: export Loro updates, import a
peer's, turn the diff into ops (plan M4).

Sync went the other way. Peers send their ops verbatim and they land through `on_sync_ops`. Nothing
in production reads the mirror now:

- `ActorHandle::import_updates`, `export_since` and `version` are called only from
  `import_tests.rs`, in this checkout, every worktree and all of git history.
- No wire message carries Loro data. The store's saved copy is read back only to restore the mirror.
- The state decides the file's bytes (the daemon's invariant), not the mirror. The one exception,
  `adopt_mirror_rendering`, sits inside `on_import`, which nothing calls.

Same-word conflict review lives on the same dead path. `txtodo_crdt::detect` runs only in
`on_import`, which is the only producer of `needs_review` flags. So in production the stored flag
list is always empty, and the conflicts view shows only duplicate groups (ADR 0032), which come from
the state.

The mirror also cannot follow the state. Since ADR 0033 and 0034 the state orders and merges by HLC
stamp; the Loro list orders by position. They part when the two orders differ: a late placement, a
clock behind its peer, a big synced batch. The lab logs mirror errors in every chaos and clock-skew
run (`mirror_flush_disagreed_converging`, `mirror_refused_converging`,
`mirror_converge_disagreed_new_lineage`, 0 to 8 per run). They are all healed, and none changes a
file. 84704ac6 and 9fe647ec each fixed one of these, in a copy nobody reads.

## Decision
We will remove the Loro mirror of `todo.txt` documents and the import path built on it.

- Remove from the daemon: `mirror.rs`, `mirror_converge.rs`, the mirror upkeep in `actor_mirror.rs`
  (flush, converge, resync, `on_export`), `sync_ops.rs`'s `mirror_after_parking`, `on_import` and
  `ActorMsg::Import`/`Export`/`Version` with their handle methods, and `CommitTail::flush` and
  `persist_mirror`.
- Stop reading and writing the store's mirror row for `todo.txt` files. Old rows stay and are
  ignored; a later store migration may drop them.
- Keep the `notes.md` mirror (`NotesDoc`). It is the merge engine for notes, used by the held-save
  three-way merge (`notes_held.rs`), and it is read.
- `txtodo-crdt` keeps what `NotesDoc` needs and drops the list code with no other caller.
- ADR 0002 stands: Loro is still the engine where we use one (notes). This narrows where it is used,
  not which engine.
- No wire change and no protocol bump: the mirror never reached the wire.

Same-word conflict review (decided 2026-10-02: A):

- A: drop it with the mirror. The conflicts view keeps duplicate groups only, which is what users
  see today. The `needs_review` table and `Resolve`'s Mine/Theirs go too.
- B: rebuild it on the op path. The state's text history (ADR 0034) already sees two concurrent
  `EditText`s on one description and could raise a flag there. That needs its own ADR.
- The owner chose A: it goes with the mirror. B stays open as its own ADR if same-word review is
  wanted later.

## Consequences
- Good: the mirror errors stop, and the lab's mirror tripwire can go.
- Good: about 1 600 lines leave the daemon (`mirror`, `mirror_converge`, `actor_mirror`, `import`
  and their tests), plus the list code in `txtodo-crdt`.
- Good: no full-list agreement walk on commit or export, and no Loro document per open `todo.txt`.
  Whether this helps the idle RSS gap (`tests/e2e/idle_rss.rs`, about 1.7 GB against 50 MB) is not
  measured yet.
- Bad: syncing `todo.txt` as Loro updates is no longer an option without building it again. The web
  PWA plan (`tasks/web-pwa`) already syncs with op messages (Hello/Want/Ops), so nothing planned
  needs it.
- Bad: old stores keep a dead mirror row per `todo.txt` until a migration drops it.
- Neutral / follow-ups: `scripts/lab/lib/checks.sh` drops `mirror_refused_converging`; the daemon's
  CLAUDE.md invariants and ADR 0036's "the Loro mirror applies it" line no longer apply.

## Alternatives considered
- Keep the mirror and fix each gap: make `converge_to` reach the state for big batches, and log the
  expected gaps (clock skew, late placements) quietly. It keeps paying, on every commit, for a copy
  nobody reads, and each new state rule (as 0033, 0034 and 0036 were) needs a matching mirror rule.
- Keep the mirror but stop checking it: the errors go quiet, but the export would ship a document
  known to differ from the file the day anything reads it.
