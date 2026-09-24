# tui-prompt

## Goal
- Quick Add as an always-visible prompt bar, like c2.

## Design
- It reuses the `ui/edit.rs` single-line editor.
- Enter sends `Add` to the current workspace root. The daemon stamps the date and `id:`.
- Chips go through core `edit::*`. Strict hints go through core `lint::strict_hints`.
- Focus key: `Ctrl-Space`, plus `Ctrl-Shift-Space` under enhanced keys.
  - This `differs` from desktop's `Mod-Shift-Space`, and the manifest records that.

## As built
