# tui-tasks

## Goal
- The Tasks screen matches the c2 buffer view, in list mode.

## Design
- Rows keep one `LineState` per line.
  - The gutter, done styling, `ref:` n/m and the 100-char underline follow implementation-plan §3.1.
- Search never hides or reorders rows.
  - Hits are reversed, misses faded, and the current hit is tinted.
  - Enter / Shift-Enter cycle through hits.
  - It uses core `query::matches`.
- The conflict sheet replaces the `r` pane.
  - It reuses `ui::conflicts::resolve_request` and shows a core diff (as `txtodo-ffi` `diff_view.rs` does).

## As built
