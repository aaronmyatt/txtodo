# 0030 — A workspace's ref folder is settable, and defaults to `tasks/`

- Status: accepted
- Date: 2026-09-21
- Deciders: project owner (the two decisions below, 2026-09-20)
- Supersedes: ADR 0012 in part (its "beside the file" placement of the root list's ref dirs)

## Context
ADR 0012 put a line's notes and sub-list in a directory named by `ref:<slug>` beside the file that
holds the line, and said not to relitigate it. In a project's own workspace that scatters one folder
per task through the repository root next to `crates/` and `docs/`, and `prune --orphans` then has
to treat every root-level folder as a possible orphan. The owner reopened it on 2026-09-20.

## Decision
The workspace layout has two settings, in `<root>/txtodo.toml`:

- `refs_dir`: the folder for the ref dirs of lines in the **root list**, relative to the workspace,
  default `tasks`. `.` puts them beside the list, ADR 0012's layout, for a workspace that sets it.
- `todo_file`: the root list, default `todo.txt`.

A line in any other list keeps its ref dirs beside its own file, so nesting is unchanged
(`tasks/q4-roadmap/todo.txt` with `ref:sync-section` gives `tasks/q4-roadmap/sync-section/`).

- **Where it is stored.** A `txtodo.toml` at the workspace root: visible, diffable, in git with the
  workspace. The daemon gains the `toml` crate (already in the tree through txtodo-cli).
- **Existing workspaces.** Neither pin nor migrate: the default flips to `tasks/` for every
  workspace. The project is young and nothing else syncs these workspaces yet, so a one-off script
  (`scripts/migrate-refs-to-tasks.sh`) covered this machine, and found nothing to move.
- **One place.** `WorkspaceLayout::ref_dir_for` (`txtodo-model`) is the one place a slug becomes a
  directory. Ref-dir creation, the `RefDir` and notes RPCs, cross-file moves, the workspace tree and
  `prune` all use it; clients ask the daemon (`WorkspaceLayout` RPC) rather than guessing.
- **Validation.** Relative, `/` separators, no `..`, no `:`, not under `.txtodo`.
- **Changing it.** The daemon reads the file at open and reloads it when it changes. A bad or deleted
  file keeps the last good layout. A change is refused while ref dirs sit in the old place, unless
  the caller asks to move them (`txtodo workspace layout --refs-dir P --move`), which is
  all-or-nothing.
- **Prune** offers only directories inside `refs_dir` (all of them when refs are beside the list),
  so `crates/` and `docs/` are never orphan candidates.

## Consequences
- A workspace made elsewhere with ADR 0012's layout loses sight of its ref folders until it sets
  `refs_dir = "."` or moves them. The daemon keys a document's history by its path, so a moved folder
  starts a fresh history; the bytes are untouched.
- `todo_file` other than `todo.txt` is validated and stored but refused: the watcher, the walker and
  every client name `todo.txt`. Honouring it is follow-up work.
- `txtodo.toml` is not yet synced between devices, so two devices can hold different layouts. Notes
  have the same limit today (a `notes.md` edited on disk is not an op).
- The desktop refetches the layout when the workspace changes, not when the file does.
- `specs/ref-directories.md` rule 2 and plan section 3.2 changed together.

## Alternatives considered
- A registry row carried in the pairing offer: a later change on one device would never reach the
  others.
- Pin what is on disk at first open, or a `txtodo refs migrate` command: more machinery than a
  young project with one machine needs.
