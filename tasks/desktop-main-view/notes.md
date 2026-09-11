# Main view: read-only CM6 bound to Watch with decorations (plan M7, plan §3.1)

## Goal

Render `todo.txt` as a syntax-highlighted, read-only CodeMirror 6 `EditorView` with real line
numbers, bound to the daemon's gRPC `Watch` stream so an edit from any device repaints in place.
The main view never mutates the document — every write goes through the popover's `Apply`.

## Design

- Token colours per plan §3.1: `priority`, `date`, `completion-marker`, `project`, `context`,
  `tag-key`, `tag-value`, `id-tag`, `text`. Completed lines: whole line muted, description struck
  through, `x` and dates not struck. Blank lines are shown and numbered.
- `id:` tags are hidden by default: render them as a zero-width CM6 widget decoration plus a header
  toggle that shows them. They are always present in the edit field (popover).
  Ref: CodeMirror 6 https://codemirror.net/
- `ref:` lines show a trailing `n/m` indicator (open/total, plan §3.2.5) or a notes icon when the
  directory has only `notes.md`. Indicators are decorations, never text in the document.
- Hover reveals a pencil affordance on the line (desktop); clicking it opens the edit popover.
- The last line is always an empty "Add a line" row. Typing into it calls `Apply(Add)`; the daemon
  stamps the creation date and `id:` (never the UI).
- Data flow: `ListFiles` → `GetFile(workspace todo.txt)` → tokenize with the Lezer grammar
  (`desktop-lezer-grammar`) → rebuild the editor state on each `Watch` `Change` hash. Keep the
  per-line task ref (line number + `id:`) so the popover and detail view know what to open.

## Acceptance

- A 10 k-line file paints with startup-to-first-paint ≤ 500 ms on the CI runner.
- A second-daemon edit flips the line in place via `Watch`, no reload.
- `id:` tags hidden by default, toggle reveals them; `n/m` and notes indicators appear only where
  `ref:` directories exist.

Refs: plan M7 and §3.1–3.2 (txtodo-implementation-plan.md), design §7 (txtodo-design.md).
