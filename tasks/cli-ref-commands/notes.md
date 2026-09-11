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
