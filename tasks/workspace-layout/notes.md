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
- Order: the daemon resolves against a `WorkspaceLayout` it is handed, and falls back to
  `legacy()` (beside the file) until the pin-at-first-open line lands. Only then does a new
  workspace get `tasks`. That way no existing workspace loses its notes in between.
- `prune` must scan `refs_dir`, not the whole root. Today's scan of root-level dirs would call
  `crates/` and `docs/` orphans once refs live in `tasks/`.
- Changing the layout of a live workspace moves every ref dir with the same collision rule as a
  cross-file move (`move_ref_dir`), or is refused. Setting it without moving would orphan them all.

## Decisions waiting (the two Decide lines)

1. Where the layout is stored. A: `txtodo.toml` at the workspace root, synced like `notes.md`; the
   `toml` crate is already in the tree through txtodo-cli, and adding it to the daemon needs a
   Cargo.toml and `deny.toml` check. B: a registry row carried in the pairing offer; a later change
   on one device does not reach the others. I'd take A.
2. Existing workspaces when the default flips. A: at first open, find where the ref dirs really are
   and write that down (`.` if beside the list, `tasks` if there); a new empty workspace gets
   `tasks`. This repo needs no setting. B: the daemon looks in `tasks/` everywhere and a
   `txtodo refs migrate` moves the old dirs. I'd take A: nothing moves on anyone's disk unasked.

## Known gaps

- Pin race: a fresh device that opens before sync arrives has no ref dirs, so it could write
  `tasks` while the device with real dirs wrote `.`. The pin should be written only by a device
  that has dirs, and an empty one waits for sync. Not solved here.
- `todo_file` in a subdirectory (`lists/todo.txt`) with `refs_dir = "."` puts refs in `lists/`.
  That follows rule 2, but it is untested.
- `apps/desktop` and the TUI cache the root path; a live layout change needs a reload signal.
