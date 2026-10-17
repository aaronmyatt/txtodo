# Fix the desktop's vim-style list keys

## Goal

The Tasks list's single keys (`j`/`k`, `g g`, `G`, `i`/`a`, `x`, `d d`, `J`/`K`, `u`, Enter;
`specs/client-parity.toml` list.*, `apps/desktop/src/lib/keys.ts`) should work the way the TUI's
do. Today they stop working, or type into the file, in common cases.

## What is broken (2026-10-17, agent triage)

1. **A click in the list takes the keyboard.** The list is a live CodeMirror buffer, so a click
   focuses it and puts a caret there. List mode needs nothing focused (`AppShell.svelte`
   `onKeydown`), so `j` then types a `j` into the line, and blur saves it. Only Esc gets back.
   The manifest calls this out (`list.click_to_edit`, desktop "differs").
2. **Esc in the prompt bar focuses the buffer** (`PromptBar.svelte` `leave()`): same result, the
   next `j` is typed.
3. **A focused button blocks list mode** and Esc does not let it go (`WorkspaceSwitcher` hands
   focus back to its toggle on close; Chromium webviews keep focus on a clicked button).
4. **Shift is dropped** from named keys (`keyName` has no `shiftKey`): Shift-Space completes,
   Shift-Enter opens the detail, Shift-Up moves the caret. The TUI writes `Shift-` (keymap.rs).
5. **Mod-Enter does nothing in list mode**: the manifest's `detail.open` lists it, only the
   editor binds it.

Not changed: Esc in an editor still discards an unsaved edit (`edit.cancel`, the same in both
clients), and there is still no opt-in vim mode (rejected in `tasks/tui-revamp/notes.md`).

## Decided 2026-10-17 (agent)

- Click: the TUI's rule. The first click on a line selects it and leaves list mode on; a click on
  the line that is already current edits it, caret where clicked. A double-click still opens the
  detail. Rejected: leave the click as is and only teach Esc (the stray letters are the bug);
  a mode toggle (one more thing to explain, and the TUI has none).
- Esc on a focused button or link blurs it, so list keys work again.
- `keyName` writes `Shift-` before a named key, after `Alt-`, like the TUI.
- `detail.open` binds `Mod-Enter` too; `Chords` reads `Super-`/`Ctrl-` as `Mod-`.
- `list.click_to_edit` flips to desktop "done" in the manifest.

## As built (2026-10-17)

- All five in 2833155b. `listMode.ts::clickOnlySelects` is the rule, `FileView`'s `mousedown`
  applies it; `AppShell::releaseControl` blurs a focused button or link on Esc without stopping
  the event; `keys.ts` names `Shift-` and maps `Super-`/`Ctrl-` to `Mod-`.
- Tests: vitest (`clickOnlySelects`, `keyName` Shift, `Chords` Mod) and two new functional e2e
  cases in `e2e/list-mode.spec.ts`; `list-mode`, `escape` and `prompt-bar` specs all pass.
- Human check left: on the real app, click a line, press `j`/`k` (moves, types nothing); click the
  same line again (caret, editing); Esc; Cmd+Enter (detail opens).
- Not done: Esc in an editor still discards an unsaved edit (`edit.cancel`, kept on purpose);
  a drag-select now needs the line selected first (the first click only selects).
