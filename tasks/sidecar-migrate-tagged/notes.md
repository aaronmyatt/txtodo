# Migrate a Tagged workspace to Sidecar identity in place

## Goal

ADR 0019 made Sidecar the only identity mode, but nothing ever converted an existing Tagged
workspace. `load_or_mint_identity_mode` (`workspace_mint.rs`) picks `Tagged` on the first open if
any file already has an `id:` tag, and never re-checks. This repo's own `todo.txt` was seeded with
ids, so its `.txtodo/oplog.db` says `identity_mode = 0x00` and the daemon keeps stamping `id:` onto
every new line, including lines typed in the desktop app.

Wanted: convert such a workspace once, keep every task's identity and history, leave no `id:` tags
on disk.

## Design

- Identity survives. Sidecar identity is a `fingerprints` row keyed `(file, task)`. Reuse the ULID
  from each line's `id:` tag as that `task`, so the oplog, blame, undo and the CRDT mirror keep
  pointing at the same tasks. Dropping the store instead would orphan all history.
- Do it inside the daemon, per document actor, not as an offline file rewrite. A file rewrite
  would leave the ops, projection and Loro mirror still holding `id:` text, and the projection
  writer would put the tags straight back.
- Per actor, one commit:
  1. Take the current `DocState` (Tagged: ids read off the text).
  2. For each task line with an own tag, emit an ordinary `EditText` op that deletes the tag word
     and its leading space. Same op kind a user edit makes, so undo and history explain it.
  3. Switch the state and actor config to Sidecar.
  4. Commit with `CommitExtras::fingerprints` (`actor_mirror.rs` `fingerprints_for`) so the rows
     land in the same transaction as the ops. Sidecar `stored_ids` (`external.rs`) rebuilds ids
     from those rows on the next open, and returns `None` (which mints fresh ids) when they don't
     line up, so they must be complete.
  5. Write the projection without ids.
- Workspace level: `identity_mode` on `Workspace` is a plain field copied into every
  `ActorConfig`; it has to become settable. Write the `meta` row last, after every actor
  succeeded. A crash halfway leaves meta at Tagged with some files already stripped; re-running
  must treat a file with no tags as done, not as new tasks.
- Which tag is "own": the first whitespace-delimited word starting `id:` with a non-empty value
  (`fastid.rs` `fast_id_of`, property-tested against the parser). Prose like "the id: tag" and ids
  quoted inside descriptions (e.g. `(id:01M2B4...)` in parentheses) are not own tags and stay.
- Rejected: flip meta and let the reconciler re-identify by fingerprint after stripping files.
  Short lines would fall under `MATCH_THRESHOLD` and mint new ids, losing history for exactly the
  lines that changed most.
- Rejected: an offline `txtodod migrate` binary. One global daemon serves many workspaces and
  holds the store open.

## Known gaps

- Single device only. Pairing does not propagate `identity_mode` (docs/questions.md Q6, dissolved
  by ADR 0019), so a paired peer that is still Tagged will keep sending `id:` text. Migrate every
  device in the group before the follow-up that deletes Tagged.
- Migration rewrites every task line in every document, so git sees one big diff. The human
  reviews and commits it.
- The daemon that owns this repo currently takes minutes to cold-start under load (1269 documents,
  including worktree copies). Testing against the live workspace needs a healthy daemon.

## As built

Built 2026-09-19. Everything except the human step (item: install, dry-run, run, commit).

- `id_strip.rs`: `strip_own_id` wraps `Edit::remove_tag("id")`, gated on `fast_id_of`.
- `migrate_sidecar.rs`: `FileActor::on_migrate_to_sidecar` and `ActorHandle::migrate_to_sidecar`.
  One commit per document, `EditText` ops as `Principal::User`, fingerprints in the same store
  transaction. The edits apply to a Sidecar copy of the state, because a Tagged state refuses any
  edit that removes the tag (`IdMismatch`).
- `stored_ids.rs` (moved out of `external.rs` for the file budget): a Sidecar document with no
  fingerprint rows but with `id:` tags takes its ids off the tags. This is what makes "meta flips
  first, then documents migrate" safe after a crash.
- `workspace_migrate.rs`: `Workspace::begin_sidecar_migration` (writes `meta` first, returns the
  actors) and `migrate_documents` (lock-free, one document at a time, failures collected).
- `migrate_grpc.rs`, proto `MigrateIdentity`, `client_identity.rs`, `commands/identity.rs`:
  `txtodo identity migrate [--dry-run] [--yes]`. The real run always dry-runs first and asks the
  human to type `migrate`.
- Duplicate ids. Found by the rehearsal, not designed in: two lines in one document with the same
  `id:` (the root todo.txt had one, `06G9TNVQ...`, lines 40 and 69). Every op by that id lands on the
  first line and the Loro mirror cannot converge from it. The first line keeps the id, later ones
  get a fresh one, and because ops cannot express that the document commits its final state,
  pins a snapshot and rebuilds its mirror (`Migrated::renumbered`, `MigrateIdentityResponse.renumbered`).
- Re-running is a no-op for a document that is Sidecar and has fingerprint rows. It is not "no own
  tags left": once a real tag is gone, a prose mention of another `id:<ULID>` reads as the line's tag.

Rehearsal: a copy of every tracked `todo.txt`/`done.txt` (170 files) under a fresh debug `txtodod`.
1766 tags stripped, 1 line renumbered, 0 failures, every output line equal to its input with the
first valid `id:` word removed, `identity_mode` stays `01` across a daemon restart, no mirror errors
in the log. Wall time about 1.7 s.

## Still open

- Not run against this repo's live workspace. The running daemon and `/opt/homebrew/bin/txtodo`
  are older builds without the RPC.
- `mcp-cli-only-allowlist` (root todo.txt) now names `identity migrate` as a CLI-only command.
- The two lines in `tasks/security-m8-review/todo.txt` that quote a second `id:<ULID>` in prose
  keep it. Harmless in Sidecar mode; the dry-run count on a migrated workspace no longer reports
  them because migrated documents are skipped.
- Every other device in the sync group must migrate too before the follow-up removes Tagged.
