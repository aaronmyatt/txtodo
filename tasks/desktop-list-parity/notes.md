# Desktop: every list behaves the same

## Goal
- Root list and every `ref:` sub-list act the same: keys, add input, drag, undo.
- Asked by the human 2026-10-10.

## Known differences (from tasks/desktop-ui-revamp/notes.md, 2026-10-07)
- List mode (`listMode.ts`) runs on the root buffer only.
- Root list has no "Add a line…" input; sub-lists keep theirs. The prompt bar adds to the root.
- Others may exist; the audit line finds them.

## Out
- Universal's nested `ref:` sub-lists stay a deliberate cut (tasks/desktop-universal-view/notes.md).

## Open questions
- Add input: on every list, or on none (prompt bar only)? Human call.

## Audit (2026-10-11)
Both lists are the same `FileView.svelte`; the root passes `fill reportStatus listCommands
onDirtyChange`, a sub-list (DetailView) passes only `depth`. Every difference comes from that:

| Behaviour | Root | Sub-list | Why |
|---|---|---|---|
| List-mode keys (j/k, x, d, J/K, e/A, u) | yes | no | `listCommands` prop; `listCommands.ts` sends each key to every listener, and AppShell only runs them on `/` with body focus |
| Undo toast after x/d | yes | no | comes from list mode (`offerUndo`) |
| `u` undo | yes | no | list mode |
| Add input | prompt bar (lands on top) | "Add a line…" above the list | `depth > 0` branch |
| Footer status (save state, caret, counts) | yes | no | `reportStatus`; one footer, one buffer |
| Header search highlights + hit count | yes | no | `reportStatus` gates the search effects |
| Quick-add dirty guard | yes | no | `onDirtyChange`; only the main window's buffer matters there |
| Layout (fill vs capped card) | fill | card | `fill`; deliberate, a sub-list sits among detail sections |
| Drag by line number, Mod-Enter, Esc, blur/Cmd-S save | same | same | shared extensions |

Real gaps to fix: list-mode keys, undo + toasts, search highlights on the sub-list. Footer status,
quick-add guard and layout stay root-only on purpose.

The list-mode catch: with a detail panel open both buffers listen. Commands need one *active*
list: the one the human last clicked or moved into. Plan: `listCommands.ts` keeps an active
listener (last focused/clicked FileView wins, root by default) instead of broadcasting.

## List mode on sub-lists (2026-10-11)
- `listCommands.ts` now tells plain listeners (sync indicator) from list buffers. Every FileView
  is a buffer; only the active one hears list keys: the newest mounted, or the last one clicked or
  focused. Releasing the active one hands keys back to the one before.
- So Enter on a root line opens its detail and j/k/x/d/u act on the sub-list; Esc (page) closes
  the panel and the keys are the root's again. A click in either list moves the keys there.
- Rejected: "click a sub-list line, then Esc to leave the editor" as the way in. Esc from any
  editor inside the panel closes the panel (2026-10-06 feedback, e2e/escape.spec.ts), so it can't
  also mean "stay here in list mode".
- The `listCommands` prop on FileView is gone; every list registers.
- Only the active list draws its current-line highlight (class `active-list`), so the eye can tell
  which list the keys move.
- Tests: `listCommands.test.ts`, and e2e/detail-panel.spec.ts "list keys act on an open detail's
  sub-list, then on the root again". Known gap: e2e/visual/detail-view.spec.ts not re-run; its root
  buffer loses the current-line tint while the detail is open, so its snapshot may need updating.

## Rest of the audit (2026-10-11)
- Search: every list now marks the header search's hits and fades misses. The pill's count and
  Enter's stepping stay on the root buffer: one pill, one count. e2e: "the header search marks hits
  in the sub-list".
- Left root-only on purpose, per the audit: footer status, quick-add dirty guard, fill layout.
- Still open: the add input, waiting on the human's call (line 2).
- e2e/list-parity.spec.ts (fixture `parity`) runs j/k, J + u, x + toast Undo, d d and search once
  on the root list and once on a detail's sub-list. A new list behaviour gets its check there.
  The sub-list search check moved there from detail-panel.spec.ts.

## Decided 2026-10-11 (agent, asked to decide @human lines): the add input
- Options: (A) an "Add a line…" input on every list, root included; (B) none: the prompt bar adds.
- Pick: B, with the prompt bar adding to the active list (the root, or an open detail's sub-list
  once it has the keys). The note under the bar says "Adds to <path>" when that is not the root.
- Why: the human already took the input off the root list on 2026-10-06 (commit 10547521) in
  favour of the prompt bar; B keeps that call and gives sub-lists the same single way in, and
  `n` (prompt.focus) now reaches it from either list. A would undo the earlier call.
- Cost: adding to a sub-list now goes through the bottom bar, further from the list; the
  "Adds to" note is there so it never lands somewhere unexpected. Easy to revert: the FileView
  input lived in one `{#if depth > 0}` block.
- Built: `listCommands.ts::activeListPath`, PromptBar's target, FileView's input removed;
  e2e/list-parity.spec.ts checks n + Enter on both lists.

## From the other device's build (restored 2026-10-26)
The 2026-10-26 merge kept the remote side of this file; these lines are the local side's. Their
commit hashes are on the local side (`85756893^1`); HEAD runs the remote side's equivalents.

### Desktop: root list and sub-lists behave the same

The root line (human, 2026-10-10) asks that a sub-list (a `ref:` directory's `todo.txt`, shown in
the detail panel) behave like the root list. Both are `FileView.svelte`, but the root gets opt-in
props the sub-list does not (`fill`, `reportStatus`, `listCommands`, `onDirtyChange`;
`MainView.svelte:158` vs `DetailView.svelte:387`) and the subbar and prompt bar from outside.
The TUI already runs list mode on a sub-list (`crates/txtodo-tui/src/input_detail.rs`).

### Inventory (survey, 2026-10-13; paths under apps/desktop/src/lib/components unless noted)
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

### Open question (@human)
- Which of these are bugs and which are deliberate? The decide line lists them; the rest of the
  lines wait on it. Some are design calls (Esc closing the panel, one prompt bar for every list).
- Parity manifest gaps found on the way: `list.*` rows say "FileView (list mode)" with no note that
  desktop list mode is root-only; `detail.close` (Esc) is `done` without saying it closes every
  level.
