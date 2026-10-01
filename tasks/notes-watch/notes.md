# notes-watch

## Goal

An editor's save to a `notes.md` syncs as it happens, like a save to a `todo.txt`, and is never
lost.

## Evidence (2026-10-01, the p2p lab)

`lan-converge`, seed 830835642, both attempts after editor-save-lost: the one token still lost is
a `tasks/lab/notes.md` editor append on a1. It is in a1's file and never reaches b1: no op was
ever made for it.

## Where things stand

- Nothing watches `notes.md`. `watch_task.rs::route_document` hands an event to
  `actor_for_disk(path)`, which only knows `todo.txt` actors; a `notes.md` event falls through
  to `discover`, which does nothing for it.
- `NotesActor` has no mailbox. One writer is `notes_registry.rs`'s `Arc<Mutex<NotesActor>>` per
  path, opened lazily (`get_or_open`).
- Since editor-save-lost, `NotesActor::absorb_disk` commits foreign disk text as an `External`
  `NotesEdit` at open and before `edit`, `import_ops` and `import_updates`. So a save is never
  written over, but it only becomes an op when something else writes that file.
- `absorb_disk` reads without a debounce: a save written in place could be read half-way.
- `txtodo.toml` already has its own route in `route_document` (`layout_reload`), and it syncs
  through a notes actor (`layout_sync.rs`): the model to copy.

## Design

- `route_document`: a path with `walker::is_notes_document` goes to the workspace's notes
  registry: `get_or_open`, then `absorb_disk`. The watcher's debounce (150 ms) already applies,
  so the read is of a settled file.
- Writes: the same pre-rename check as `FileActor` (`write::write_atomic_if`): if the file is
  not our last write, hold the write and keep the base; the next event merges three-way. For
  text that is `diff(base, disk)` applied onto the current text: the editor's edits on top of
  what peers wrote meanwhile. Then `absorb_disk` before writes can go.
- A held base survives a restart the same way (`meta` key `held_base/<file>`).

## How to check

- Unit: a notes actor test where a save lands on disk and only the watcher's message follows:
  the text has an op and reaches a fresh peer.
- Lab: `lan-converge` loses no `notes.md` token on three seeds; the no-loss check already
  counts `tasks/lab/notes.md`.

## As built

- 2026-10-01, watcher route: `watch_task.rs::route_document` sends a `notes.md` event (after the
  150 ms debounce) to `absorb_notes`: `notes_actor` (get or open), then `absorb_disk` under the
  actor's lock. A missing file records nothing, so a deleted or moved `ref:` dir never syncs as
  emptied notes. `tests/editor_saves.rs::a_notes_md_save_is_taken_on_the_watchers_event` fails
  without the route. Still open: the pre-rename check and three-way merge (a peer's import can
  still read the disk mid-save before the debounce fires, through `absorb_disk` before writes).
