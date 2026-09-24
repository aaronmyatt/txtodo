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
