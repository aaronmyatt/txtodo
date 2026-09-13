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

## As built (2026-09-13, agent)

Found already built from an earlier session — `apps/desktop/src/lib/components/{MainView,FileView}.svelte`,
`$lib/todotxt/{decorations,lineInfo,editRequest}.ts` implement everything in this file's Design
section: real CM6 read-only `EditorView` per `FileView` instance, hidden `id:` tags via a
`MatchDecorator` zero-width `Decoration.replace`, completed-line muting/strike via a viewport-bounded
`ViewPlugin`, `ref:` trailing `n/m`/notes-icon widgets sourced from `ListFiles`'s own
`FileInfo.progress` (never recomputed), a hover-revealed pencil, and an always-present "Add a line"
row. `MainView` hosts the popover (per this file's own contract) and the root `ConflictBanner`.
`$lib/lib/components/__tests__/lineInfo.test.ts` covers the pure decoration-placement logic.

Extended this session, for `desktop-detail-view`'s benefit (kept in `apps/desktop/src/lib/components/FileView.svelte`,
not a new file — this component is explicitly designed to be reused recursively):

- `onDetailRequest?: (params: DetailParams) => void` prop plus a `dblclick` DOM handler and a
  `Mod-Enter` (Cmd+Enter macOS / Ctrl+Enter elsewhere — CM6's own binding convention) keymap entry,
  both resolving the line under the pointer/caret and calling `onDetailRequest({file: path, line})`.
  Fires for any line, `ref:` tag or not — a task with none still lazily gets one on its first notes
  edit (plan §3.2.4), so gating the double-click on an existing tag would just make that flow
  undiscoverable.
- No change to the file's other contracts: `onEditRequest` still works exactly as before,
  `depth`/`path` are unchanged, and the 10 k-line/500 ms budget claim wasn't touched (still
  viewport-bounded decorations only).

Bug found and fixed while wiring `desktop-edit-popover`'s reuse for `desktop-detail-view`/
`desktop-quick-add` — recorded on that task's own "As built", not duplicated here, because the
fix lives in `EditPopover.svelte` — but it directly affects this file's hosted popover: `FileView`'s
own `saveLocalEdit` (used when a caller passes no `onEditRequest`, unaffected by the fix — it was
already the *sole* place that path applies) was fine; `MainView`'s hosted case was double-applying
every edit before the fix (see desktop-edit-popover's notes for detail).

## What to open and look at

- `npm run tauri dev` from `apps/desktop` in a workspace with a `todo.txt` of a dozen or so lines,
  some completed (`x <date> ...`), one with a `ref:<slug>` tag pointing at a real
  `<slug>/todo.txt`. Check: line numbers include blanks; completed lines are muted with the
  description struck through (not the `x`/dates); the `ref:` line shows `n/m` at its right edge;
  toggling "Show `id:` tags" reveals/hides the tag inline.
- Hover a line: a pencil (✎) appears at its right edge; click it — the edit popover opens
  (`desktop-edit-popover`'s own "what to look at" covers verifying that popover itself).
- Double-click a line (or click into it and press Cmd/Ctrl+Enter): the detail view opens
  (`desktop-detail-view`'s "what to look at" covers that screen).
- Edit the same `todo.txt` from a second terminal (`echo "new task" >> todo.txt` or via the CLI)
  while the app is open: the line appears/repaints without a manual reload.
