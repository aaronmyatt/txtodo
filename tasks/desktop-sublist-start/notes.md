## Goal
- Detail view lets you start a sub-list on a task with no `ref:` yet, the way notes already can.

## Why it fails today
- `DetailView.svelte` renders the sub-list only when `todo.txt` exists with tasks (`hasSubList`).
- Notes work: `NotesEditor` -> `editNotes` -> daemon `ensure_ref_dir` on first keystroke.
- No desktop binding for the daemon `RefDir` RPC (`daemon.ts`, `src-tauri`).
- Planned in tasks/desktop-detail-view ("first keystroke into notes or sub-list"); only notes shipped.

## Design
- Chosen: show an empty sub-list with an add-line input. First submit calls `RefDir` (creates tag + dir), then adds the line via `applyMutations` into the new `todo.txt`.
- Rejected: force the ref with `editNotes("")`. Leaves a stray `notes.md`.
- Opening the view alone must write nothing (lazy creation, same rule as notes).
- New-ref path depends on the workspace layout (`refDirFor`); the daemon decides it, the UI never mkdirs.
