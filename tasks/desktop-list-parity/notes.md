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
