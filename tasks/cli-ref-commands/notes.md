# `txtodo open`, `notes`, `sub`, `prune --orphans` — M5

Plan M5, specs/ref-directories.md rules 2, 3, 4, 10, 12. In daemon mode the CLI is a client over
the socket; these four are thin wrappers over daemon RPCs, never direct file writes.

## `open <line>` — read-only, never creates

Prints the resolved ref path (rule 2: the slug names a directory beside the file holding the line).
Resolution is pure and already needed by `daemon-ref-creation`; reuse it. Lazy creation is on the
first *write*, not on read (rule 4) — so `open` on a ref-less or dangling line must print the
*would-be* path and write nothing. Assert the negative: `open` performs no filesystem write.

## `notes <line>` — open `$EDITOR`, create lazily

Opens `$EDITOR` on `<ref>/notes.md` (rule 3). On a line with no `ref:`, the daemon adds the tag and
creates the directory in one op batch before the editor opens (plan M5 acceptance). The lazy
creation is `daemon-ref-creation`'s job; the CLI calls `GetNotes`/`EditNotes` (from `crdt-notes-doc`)
and shells out to `$EDITOR`. A missing `$EDITOR` is a clear error, not a panic.

## `sub <line> <cmd>` — a scoped todo.sh (rule 12)

`txtodo sub 2 ls` runs `ls` with the todo dir set to the line's ref directory — the same thing
`todo.sh -d <ref>/todo.cfg` does (rule 12). Implementation: resolve the line to a ref dir, then
re-exec the CLI with `--dir <ref-dir>` (or set `TODO_DIR`). Preserve every existing command and
flag unchanged; `sub` only scopes the directory.

## `prune --orphans [--yes]` — list only, delete only with --yes

Rule 10: deleting a line never deletes the directory; `prune --orphans` lists directories no line
points to and deletes them only with `--yes`. Needs the tree model
([model-workspace-tree](../model-workspace-tree/notes.md)) to know which directories are pointed to
— a *dangling* tag counts as "pointed to", so an orphan is a directory with no tag anywhere, not a
directory with a dangling tag. Without `--yes`, print the list and exit; never delete.

## Errors

A line number out of range, a blank line, or a line with no `ref:` (for `notes`/`sub`) are typed
CLI errors carrying the line number, matching the existing CLI error style — not panics.

## Tests

- `open` on a ref-less line prints the would-be path and performs no write (negative space).
- `notes` on a ref-less line adds the tag and creates the directory in one op batch (shared with
  the M5 acceptance test in [test-m5-acceptance](../test-m5-acceptance/notes.md)).
- `sub 2 ls` lists the sub-list (rule 12), byte-compatible with `todo.sh -d`.
- `prune --orphans` lists an orphan and refuses to delete without `--yes`; with `--yes` it deletes.

## As built (2026-09-13, agent)

All four in `crates/txtodo-cli/src/commands/refdir.rs`, daemon-only (direct mode returns the
existing `NEEDS_DAEMON` message, same as `log`/`blame`/`conflicts`), never a direct file write —
each is a thin wrapper over the daemon RPCs `proto-tree-progress` added
(`RefDir`/`GetNotes`/`EditNotes`/`PruneOrphans`), reusing `daemon-ref-creation`'s already-built
lazy-creation and `crdt-notes-doc`'s already-built notes actor, not reimplementing either:

- `open ITEM#`: `RefDir{ensure: false}`, prints `ctx.paths.dir.join(info.dir)`. Never creates
  anything by construction — the read-only RPC path (`refdir_ops.rs::resolve_ref_dir`) has no
  filesystem-writing code in it at all, so this isn't just a convention being followed, it's not
  possible to violate accidentally.
- `notes ITEM#`: `RefDir{ensure: true}` runs *first* — one op batch, before `$EDITOR` opens, per
  the M5 acceptance wording — then `GetNotes`/`EditNotes` around a real `$EDITOR` invocation on a
  scratch `notes.md` (`tempfile`). A missing `$EDITOR` is `CliError::Message`, not a panic. If the
  editor exits non-zero or the buffer is unchanged, no `EditNotes` call is made.
- `sub ITEM# COMMAND...`: resolves with `RefDir{ensure: false}` first and returns a typed error
  ("has no ref: tag; run `txtodo notes ITEM#` first") when `has_ref_tag` is false — matching the
  Errors section's literal wording rather than lazily creating (that's `notes`'s job, per rule 4's
  "notes *or* sub-list" reading — see the design call below). When a tag exists, `RefDir{ensure:
  true}` self-heals a dangling one (rule 9) — safe because a tag already exists, so `ensure` can
  only ever confirm the directory, never mint a new tag — then re-execs
  `std::env::current_exe()` with `--dir <ref-dir>` and the given command/args, inheriting stdio.
  Since only the workspace root runs a `txtodod` (there is no per-directory daemon), the re-exec
  transparently lands in direct-file mode there; the parent workspace's daemon picks up any
  resulting write through its existing recursive filesystem watch and `Workspace::discover`
  adopt-on-write path (`watch_task.rs`) — no new daemon-side code needed for this to work.
- `prune --orphans [--yes]`: `--orphans` is required (usage error otherwise, since it's the only
  supported mode today); `PruneOrphans{execute: yes}`.
- `client.rs` gained `ref_dir`/`get_notes`/`edit_notes`/`prune_orphans`. The `Command` enum moved
  to `cli_command.rs` (verbatim) purely to keep `main.rs` under its file-length budget after the
  four new variants and dispatch arms.
- Tests: `commands::refdir::tests` (line-ref parsing), plus an end-to-end test in
  `crates/txtodo-cli/tests/nested_ref_sync.rs` (real `txtodod`, `notes` with a no-op `$EDITOR`,
  `sub add`/`sub ls`, and `todo.sh -d` agreeing in both directions) — that file is
  `test-nested-ref-sync`'s, not this task's, but it's the one that actually exercises `sub`'s
  re-exec end to end.

## Design call the human should double check: `sub` on a ref-less line errors, it does not create

This task's own notes read two ways: rule 4 lists "sub-list" as a lazy-creation trigger, but the
Errors section literally lists "a line with no `ref:` (for `notes`/`sub`)" as a typed-error case.
Since `notes`'s own section is unambiguous about lazy creation, this session read rule 4's
"sub-list" as being about *notes* being the thing that gets written first (open a task's detail,
its sub-list included, from the same lazily-created directory), and made `sub` itself require an
existing tag — erroring with a pointer to `notes` rather than silently minting a slug from the
command line's raw args. If a human's mental model was "`sub 2 add ...` alone should also create
the ref: directory", swapping `sub`'s check for an `ensure: true` `RefDir` call unconditionally is
a small, contained change (drop the `has_ref_tag` guard in `refdir.rs::run_sub`).
