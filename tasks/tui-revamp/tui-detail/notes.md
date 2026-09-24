# tui-detail

## Goal
- The detail view (breadcrumb, parent line, sub-list, notes) as a bottom split panel.

## Design
- The panel takes 55% of the height and the list stays visible, like c2.
  - Desktop's panel-vs-page choice is still open (desktop-ui-revamp decide line 2). The TUI takes the panel, pending the human decide line.
- A stack of levels, like `MainView.svelte`'s `detail` stack. Breadcrumb clicks cut back to a level.
- The sub-list uses `RefDir(ensure)` + `GetFile`/`Watch` on the sub path. It is the same list widget and the same list mode.
- The notes editor is new, `ui/notes_edit.rs`: multi-line, caret and line wrap.
  - It autosaves after 500 ms of no typing and on close, via `EditNotes`, like `NotesEditor.svelte`.

## As built
- Built as the panel, ahead of the @human decide line. Choosing a page instead is a layout change in `ui/screen.rs::draw_tasks` (the 55% split); the stack, parts and keys stay.
- State (`state_detail.rs`): `Detail { levels, part }`; a `Level` holds its parent, ref dir, sub-list `Doc`, `Notes` and parent draft.
- List mode on the sub-list: `with_sub_list` swaps the level's `Doc` into `AppState` (path, lines, cursor, scroll), runs the root list's command, swaps back. The review flags are taken out meanwhile: they belong to the root list.
  - The line editor also runs inside the swap while the panel has the keyboard.
- Keys: Enter / Ctrl-Enter / double-click open; Tab / Shift-Tab parts; Backspace up (differs: desktop has a back button); Esc closes (from the notes, Esc returns to the sub-list first).
- Daemon side (`app_detail.rs`): `RefDir(ensure=false)` + `GetFile` + `GetNotes` on open; `RefDir(ensure=true)` before the first Apply to a missing sub-list; `EditNotes` on the 500 ms pause (a `Wake::Autosave` arm in the loop) and on leaving a level.
  - Apply, Undo and Watch changes re-read whichever open list they name (`refetch`), then each parent line.
  - The loop watches the root list and every open sub-list that exists.
- Gaps: no `ref:` badges inside the panel; notes lines are cut, not wrapped; a flagged line inside a sub-list is not read-only (flags are tracked for the root list only).
- Tests: unit tests per module; `tests/detail_panel.rs` drives add, drill, notes, reopen against a real daemon.
