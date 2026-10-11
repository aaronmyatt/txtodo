# Drop images into a task's notes

## Goal
- In the desktop notes editor, drag an image file in (or paste one): the image is kept with the
  task and shows in the notes.
- Asked by the human 2026-10-11.

## Today
- Notes are a CodeMirror markdown source editor (`NotesEditor.svelte`), saved through the daemon
  (`editNotes`, a Loro NotesDoc). No rendered preview, so an image link alone shows nothing.
- The daemon owns the files and syncs text documents only (todo.txt, notes.md). No path carries
  bytes of any other file to another device.

## Design
- Store: `tasks/<slug>/assets/<hash8>-<name>.<ext>` next to notes.md, linked from notes as
  `![name](assets/<hash8>-<name>.<ext>)`. Plain markdown, so it reads in any editor and on GitHub.
- Show: a CodeMirror widget decoration renders the image under its `![..](..)` line.
  Ref: https://codemirror.net/docs/ref/#view.Decoration^widget
- Write: who writes the bytes, and how they reach other devices, is the open call below.

## Open questions (human)
- How do the bytes get written and synced?
  - A: the desktop writes the file itself (Tauri fs), local only; other devices see a broken
    link until a later blob-sync task. Small, no wire change.
  - B: a daemon `PutAsset` RPC plus blob sync over LAN/relay (new proto messages, an ADR). Real,
    but the size of a sync feature.
  - Pick: A now, B as its own ADR later. Waiting on the human because B is a wire-format change.

## Decided 2026-10-11 (agent, asked to decide @human lines): option A
- A: the desktop writes the image itself into `tasks/<slug>/assets/`, local to this device.
- Why A: it gives the human the feature now with no wire change; the files sit in the workspace
  as plain files and markdown, so any later blob sync (B) can carry them without changing the
  notes. B is a sync feature (new proto messages, size limits, an ADR) and stays its own task.
- Safety: only an `assets/` folder beside a `notes.md` inside the open workspace is ever written
  or read; names are cleaned to `[A-Za-z0-9._-]`; png/jpg/gif/webp only (no SVG: it can carry
  script); 10 MB cap. The file name starts with an FNV-1a hash, so one image dropped twice is one
  file and two images with one name never collide.

## As built (2026-10-11)
- Rust `commands_assets.rs`: `save_note_asset`, `read_note_asset` (also served by the e2e bridge).
- `$lib/notes/noteImages.ts`: a StateField of block widgets draws each `![..](assets/..)` image
  under its line through a blob URL; drop and paste handlers in `NotesEditor.svelte` save the
  file and insert the markdown line. A task with no notes yet gets its notes saved first, which
  makes the folder.
- e2e/notes-images.spec.ts pastes a PNG and checks the file, the link and the drawn image.
- Known gaps: images do not sync to other devices (another device shows the alt text with "not on
  this device"); deleting the link leaves the file in assets/.
