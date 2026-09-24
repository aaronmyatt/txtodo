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
- `prompt.rs` (logic) and `ui/prompt_bar.rs` (drawing). The draft is an `EditDraft` in `Shell`, kept while the bar loses the keyboard.
- Enter adds to `state.path`, the root list, even with the detail panel open (the sub-list is only swapped in for list commands). Enter keeps the keyboard, so several lines go in a row; an empty Enter or Esc leaves.
- Chips: core `chips::apply_chip`, by click or `Alt`+`a b c x p o d t r`. Alt needs Option-as-Meta in macOS terminals.
- Strict hint: core `strict_hint`, 150 ms after the last key. The loop's `Wake::Deadline` covers both this and the notes autosave.
- Test: `tests/prompt_bar.rs` (Ctrl-Space, a chip, Enter, the toast).
