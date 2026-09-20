# workspace-layout

## Goal

Decided 2026-09-20 (it replaces the old "sub/open resolve a line's ref:" line and picks neither of
its options): a workspace's root list is `<root>/todo.txt` and the folders for its notes and
sub-lists are `<root>/tasks/<slug>`. Both places are settable per workspace.

- `todo_file`: the root list, relative to the workspace root. Default `todo.txt`.
- `refs_dir`: the folder for the ref dirs of lines in the root list. Default `tasks`. `.` means
  beside the list file, which is ADR 0012's layout, kept for workspaces that already use it.

This reverses the default of ADR 0012 ("do not relitigate"; the owner did, on 2026-09-20). The new
ADR supersedes it in part. Rule 2 of `specs/ref-directories.md` and plan section 3.2 change with it.

## Design

- Only lines in the root list use `refs_dir`. A line inside `tasks/<slug>/todo.txt` still resolves
  beside its own file (`tasks/<slug>/<sub-slug>`), which is how this repo already nests. Nested
  lists and notes keep the names `todo.txt` and `notes.md`.
- One daemon helper turns (owner file, slug) into a directory. Today that logic is in
  `refdir_grpc.rs::ref_dir_path`, `refdir_ops.rs::own_dir` and `notes.rs::ref_notes_path`, and
  `move_coordinator.rs` relocates dirs on a cross-file move. All four go through the helper.
- Clients never read the layout themselves. They ask the daemon (`WorkspaceInfo`), so the CLI
  stops passing a hard-coded `"todo.txt"` to `RefDir` (`commands/refdir.rs`) and the desktop stops
  using `ROOT_PATH` as a constant.
- Validation: a relative path, `/` separators only (paths sync across operating systems), no `..`,
  no drive letter, not under `.txtodo`. Fuzz these like slugs.
- The layout decides synced paths, so it has to be the same on every device. A device-local setting
  could put one task's folder in two places. That is why where it is stored is a decision.
- Order (changed 2026-09-20, see Decided): a workspace with no `txtodo.toml` gets the defaults,
  `todo.txt` and `tasks`. There is no `legacy()` fallback and no pin at first open; the one-off
  script moved (or found nothing to move on) this machine before the default flips.
- `prune` must scan `refs_dir`, not the whole root. Today's scan of root-level dirs would call
  `crates/` and `docs/` orphans once refs live in `tasks/`.
- Changing the layout of a live workspace moves every ref dir with the same collision rule as a
  cross-file move (`move_ref_dir`), or is refused. Setting it without moving would orphan them all.

## Decided

- 2026-09-20, where the layout is stored: A. A `txtodo.toml` at the workspace root, synced like
  `notes.md`. A change on one device reaches the others, and the file is visible, diffable and
  lives in git with the workspace. Costs accepted: the daemon gains the `toml` crate (already in
  the tree through txtodo-cli; `Cargo.toml` is a frozen path, and `cargo deny check` still has to
  pass), and a new file type enters sync, so the walker, the watcher and the sync path each have
  to learn it. B (a registry row carried once in the pairing offer) was rejected: a later change
  on one device would never reach the others.

- 2026-09-20, existing workspaces when the default flips: neither A (pin what is on disk at first
  open) nor B (a `txtodo refs migrate` command). The project is in its early days and no other
  device syncs these workspaces yet, so the default simply flips to `tasks/` for every workspace,
  and a one-off script updates this machine: `scripts/migrate-refs-to-tasks.sh` (dry run by
  default, `--yes` applies, refuses while a `txtodod` runs). Run on 2026-09-20 it finds nothing to
  move: all 13 registered workspaces already keep their refs under `tasks/` or have none.
  What this removes from the plan: the pin-at-first-open line, the `legacy()` fallback the Design
  section's "Order" bullet describes, and the pin race under Known gaps. `refs_dir = "."` stays a
  valid value for a workspace that sets it in `txtodo.toml`.
  Cost accepted: a workspace made elsewhere with ADR 0012's layout would lose sight of its ref
  folders until the script (or a hand move) runs there. The daemon keys a document's history by
  its path, so a folder the script moves starts a fresh history; the file bytes are untouched.

## Decisions waiting

None. Both Decide lines are closed.

## The two Decide lines as they were asked

1. Where the layout is stored. A: `txtodo.toml` at the workspace root, synced like `notes.md`; the
   `toml` crate is already in the tree through txtodo-cli, and adding it to the daemon needs a
   Cargo.toml and `deny.toml` check. B: a registry row carried in the pairing offer; a later change
   on one device does not reach the others. I'd take A.
2. Existing workspaces when the default flips. A: at first open, find where the ref dirs really are
   and write that down (`.` if beside the list, `tasks` if there); a new empty workspace gets
   `tasks`. This repo needs no setting. B: the daemon looks in `tasks/` everywhere and a
   `txtodo refs migrate` moves the old dirs. I'd take A: nothing moves on anyone's disk unasked.

## Known gaps

- (Gone with the 2026-09-20 decision: the pin race. Nothing is pinned any more.)
- `todo_file` in a subdirectory (`lists/todo.txt`) with `refs_dir = "."` puts refs in `lists/`.
  That follows rule 2, but it is untested.
- `apps/desktop` and the TUI cache the root path; a live layout change needs a reload signal.
