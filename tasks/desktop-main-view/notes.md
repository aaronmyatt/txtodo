# Main view: read-only CM6 bound to Watch with decorations (plan M7, plan §3.1)

## Goal

Render `todo.txt` as a syntax-highlighted, read-only CodeMirror 6 `EditorView` with real line
numbers, bound to the daemon's gRPC `Watch` stream so an edit from any device repaints in place.
The main view never mutates the document — every write goes through the popover's `Apply`. This is
the file-as-UI-model rule (design §7): highlight from `tokenize`/Lezer spans, never re-parse.

## Design

The view holds a CM6 `EditorState` whose `doc` is the raw `GetFile` bytes, and a set of
decorations recomputed per `Change`:

```ts
// apps/desktop/src/main/main.ts
type TaskRef = { line_number: number; id?: string };          // id absent pre-stamp
function buildState(bytes: string): EditorState;               // one EditorState per file
function decorationsFor(view: EditorView, tree: TreeProgress): DecorationSet;
function paintChange(change: Change): void;                    // rebuild from GetFile bytes
```

- **Token colours** (§3.1): `priority`, `date`, `completion-marker`, `project`, `context`,
  `tag-key`, `tag-value`, `id-tag`, `text` — mapped in one theme file to colours so every platform
  shares the mapping. Completed lines: whole line muted, description struck through, `x` and dates
  not struck. Blank lines are shown and numbered (blanks are entries — `core-parse-file`).
- **Hidden `id:` tags**: rendered as a zero-width `Decoration.widget` (CM6
  https://codemirror.net/docs/ref/#view.Decoration%5Ewidget) plus a header toggle that shows them.
  They are always present in the edit field (the popover).
- **`ref:` indicators**: lines whose directory has a `todo.txt` show trailing `n/m` (open/total,
  plan §3.2.5); lines whose directory has only `notes.md` show a notes icon. Both are
  `Decoration.widget` decorations, never text in the document. `n/m` comes from
  `ListFiles`/`Watch` `Progress { done, total }` (`proto-tree-progress`), not recomputed client-side.
- **Hover pencil**: a `ViewPlugin` (https://codemirror.net/docs/ref/#view.ViewPlugin) reveals a
  pencil affordance on the hovered line; clicking it opens the edit popover with the line's
  `TaskRef`.
- **"Add a line" row**: the last line is always an empty row. Typing into it calls
  `Apply(Add { … })`; the daemon stamps the creation date and `id:` — never the UI.
- **Data flow**: `list_files` → `get_file(workspace todo.txt)` → build `EditorState` → tokenize
  with the Lezer language (`desktop-lezer-grammar`) → recompute decorations on each `watch` `Change`
  hash. Keep the per-line `TaskRef` (line number + `id:`) so the popover and detail view know what
  to open.

## Placement/dependencies

- `apps/desktop/src/main/` (Svelte 5 + CM6). Depends on `desktop-tauri-shell` (the `watch`/`get_file`
  commands), `desktop-lezer-grammar` (the language), and `proto-tree-progress` (the `n/m` counters).
- The 10 k-line first-paint budget (§7: ≤ 500 ms) is owned here and asserted by
  `desktop-visual-regression`; the view must bound decorations to the visible viewport so a
  10 k-line file costs only the visible window.

## Edge cases & invariants

- Line-number stability: decorations are positioned by document line, not by `id:` — a blank line
  keeps its number (design §2.6: blanks are entries).
- A `Watch` change for *this* file must not trigger a full `list_files` re-fetch unless the change
  carries a tree/progress invalidation (`proto-tree-progress` emits progress changes separately).
- `id:` may be absent on a hand-written line (lenient); `TaskRef.id` is optional and the popover
  handles the pre-stamp case.

## Acceptance

- A 10 k-line file paints with startup-to-first-paint ≤ 500 ms on the CI runner.
- A second-daemon edit flips the line in place via `Watch`, no reload.
- `id:` tags hidden by default, toggle reveals them; `n/m` and notes indicators appear only where
  `ref:` directories exist.

## References

- plan M7 and §3.1–3.2 (txtodo-implementation-plan.md), design §7 (txtodo-design.md)
- CodeMirror 6: https://codemirror.net/ · decorations https://codemirror.net/docs/ref/#view.Decoration
