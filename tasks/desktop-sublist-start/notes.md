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

## As built (2026-09-23)
- `RefDir` reaches the desktop: `commands_notes.rs::ref_dir` + `RefDirInfoDto` + `DaemonClient::ref_dir`, `daemon.ts::refDir`, the e2e bridge (`bin/e2e_bridge/refdir.rs`) and the browser mock (`mock/logic.ts::mockRefDir`).
- `DetailView.svelte`: the Sub-list section always renders; with no tasks it is one input. Submit = `refDir(ensure)` then `applyMutations(<dir>/todo.txt, [add])`; the daemon's `Change` + `listFiles` then swap in the FileView.
- Daemon: `Apply { Add }` on a list that does not exist yet, whose directory does, registers it (`apply_route.rs::actor_or_new_list`, tests in `tests/apply_new_list.rs`). Without this the add was `NotFound` and the client would have had to write the file itself.
- Known gaps: an empty-but-existing sub-list shows the same input (fine); no unit test of the Svelte branch, only the e2e. Running the Playwright suite on this Mac needs `TXTODO_TEST_KEYSTORE_MEMORY=1` (see the e2e line).
