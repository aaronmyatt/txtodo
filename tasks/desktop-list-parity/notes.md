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
