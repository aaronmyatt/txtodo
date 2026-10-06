This makes reading the task difficult in the desktop detail view
Let's also add a task breakdown view, perhaps a vertical view like `.mode line` in sqlite to give an alternative way to read the todo.txt syntax, particularly if there's a long task description

## Goal (2026-10-06, agent)
- The parent line in DetailView is readable at any length: no horizontal scrollbar over it.
- A field-by-field view of the parent line, like sqlite's `.mode line`, the same rows the TUI
  shows (`txtodo_core::breakdown::fields`, TUI `v`, manifest `detail.breakdown`).

## Design
- Wrap, not scroll: the parent's one-line CodeMirror gets `EditorView.lineWrapping`, so a long
  line breaks onto more rows and the horizontal scroller (and its bar) never appears. Enter still
  commits (the editor stays one logical line).
  - Rejected: hiding the scrollbar with CSS: the text would still run off the edge.
- Breakdown: `txtodo-ffi` exports `breakdown_fields(line)` (array of `{ name, value }`) over core;
  the committed `wasm-core/` is regenerated with `scripts/build-wasm-core.sh`. DetailView shows the
  rows under the parent line behind a "Fields" toggle button.
  - Desktop takes a button, not `v`: the panel's editors hold the keyboard most of the time, so a
    letter key would type. Manifest row becomes `differs` with that deviation.
