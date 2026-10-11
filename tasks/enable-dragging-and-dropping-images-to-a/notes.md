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
