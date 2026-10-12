# Desktop: root list and sub-lists behave the same

## Goal
The root line (human, 2026-10-10) asks that a sub-list (a `ref:` directory's `todo.txt`, shown in
the detail panel) behave like the root list. Both are `FileView.svelte`, but the root gets opt-in
props the sub-list does not (`fill`, `reportStatus`, `listCommands`, `onDirtyChange`;
`MainView.svelte:158` vs `DetailView.svelte:387`) and the subbar and prompt bar from outside.
The TUI already runs list mode on a sub-list (`crates/txtodo-tui/src/input_detail.rs`).

## Inventory (survey, 2026-10-13; paths under apps/desktop/src/lib/components unless noted)
Most visible first. Each is one line in todo.txt.
- List-mode keys (j/k, gg/G, i/a, x/Space, dd, J/K, u, Enter): root only. Only the root registers
  `onListCommand` (FileView.svelte:530-534); AppShell routes list keys only on `/` with nothing
  focused (AppShell.svelte:95-103).
- Esc in a sub-list editor closes the whole detail panel, every level
  (`onEditorEscape → onNavigateToLevel(0)`, DetailPanel.svelte:28; lib/escape.ts:9-13). The TUI
  uses Backspace for up one level.
- Adding: the prompt bar always writes the root file (PromptBar.svelte:40-42, 78-80); a sub-list
  has an inline "Add a line…" row (FileView.svelte:667-682) and an empty one a third "Add a
  sub-task" form (DetailView.svelte:131-147, 389-400).
- Search hits are never highlighted in a sub-list: the query only flows with `reportStatus`
  (FileView.svelte:538-543, 592-605); the match count counts root lines only.
- Status footer (unsaved/saving/saved, Ln/Col) and undo toasts after complete/delete: root only
  (FileView.svelte:103-104, 239-250, 490, 511-524).
- Duplicates banner in the panel names the parent's file, not the sub-list shown
  (DetailView.svelte:352-354).
- Opening a detail from the root while the panel is open pushes onto the stack instead of
  replacing it (MainView.svelte:36-38).
- Layout: the sub-list is a bordered card capped at 70vh inside a 55%-height scrolling panel
  (FileView.svelte:727-732; DetailPanel.svelte:44-45): a scroll area inside a scroll area.
- Less visible: `u` pops one global stack holding sub-list edits too; the quick-add dirty guard
  and the prompt bar's progress mark follow the root only; badges refresh only on mount;
  DetailView's own writes pass no workspace (`workspaceRoot: ""`, DetailView.svelte:139, 202, 219).

## Open question (@human)
- Which of these are bugs and which are deliberate? The decide line lists them; the rest of the
  lines wait on it. Some are design calls (Esc closing the panel, one prompt bar for every list).
- Parity manifest gaps found on the way: `list.*` rows say "FileView (list mode)" with no note that
  desktop list mode is root-only; `detail.close` (Esc) is `done` without saying it closes every
  level.
