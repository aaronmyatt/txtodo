# tui-text-editing

## Goal
- Every text field in the TUI is a sharp, obvious input with a real cursor.
- A click puts the cursor under the pointer, at any point in a line.
- Acceptance checks (the human's complaints, 2026-10-05):
  - Editing a list line: you can see where the cursor is.
  - Editing notes in the detail panel: you can see the field has focus and where text lands.
  - The parent line follows the click; the cursor is not forced to the end.
  - Editing a sub-line shows the input on that row, at once, not an `edit >` row elsewhere that
    blends into the list.

## As found (2026-10-05)
- List-line edit (`i`/`a`): no caret; the buffer is drawn as `edit > …` on the last row of the
  list area (`ui/screen.rs:105-127`).
- Sub-line edit: the same overlay, drawn above the detail panel, far from the row. Only `i`/`a`/`A`
  start it, so it looks like it appears "after typing".
- Parent line: a click runs `DetailEditParent`, which builds `EditDraft::for_line(.., false)`, caret
  at the end (`commands_detail.rs:74-82`).
- Notes: caret is a bold `▏` span. A click on the notes or the parent never sets
  `nav.focus = Focus::Detail`, so keys still go to the list.
- No click maps to a char offset: `hit.rs` targets are rows. The mouse is ignored while editing
  (`mouse.rs:211-213`).
- No focus borders: the panel is `Borders::TOP` only; the root list highlight stays `REVERSED` when
  the panel has the keyboard.
- Carets are byte offsets moved one `char` at a time; columns count chars, not display width.
- Notes are cut, not wrapped (the tui-detail notes said wrapped; they are not).
- `Focus::Edit` is declared (`state_nav.rs:73`) and never set.

## Decisions (2026-10-05, human)
- **In place, not a popup.** The edited row becomes a high-contrast input on its own row, with a
  real terminal cursor. The text stays where you clicked.
- **Two new direct deps for txtodo-tui:** `unicode-width` and `unicode-segmentation`. Both are
  already in Cargo.lock via ratatui. Record the sign-off on the deps line of
  `crates/txtodo-tui/CLAUDE.md`.

## Design
- `TextInput { text, caret, scroll_x }` in a new `src/text_input.rs`.
  - `caret` stays a byte offset, always on a grapheme boundary.
  - `EditDraft` and `Notes` wrap it. Start from the key handling in `ui/edit.rs` and
    `ui/notes_edit.rs`.
  - Column ↔ offset maps use display width (unicode-width), moves use graphemes
    (unicode-segmentation).
- One real cursor per frame: `Frame::set_cursor_position` for whichever field has focus, bar shape
  via crossterm `SetCursorStyle::SteadyBar`; reset the style on exit in `terminal.rs`.
  - https://docs.rs/ratatui/0.30/ratatui/struct.Frame.html#method.set_cursor_position
  - https://docs.rs/crossterm/0.29/crossterm/cursor/enum.SetCursorStyle.html
- Theme: new edit tokens in `theme.rs` (input bg/fg, accent border), light and dark, with a
  16-colour fallback.
- Hit map: a new `Target::Field { field, origin, scroll }`, pushed last so it wins
  (`hit.rs:81-88`).
- Click outside the field saves, not cancels: losing typed text is worse than an unwanted save,
  and `u` undoes it.
- A single click on the already selected row starts an edit at that point. A click on an
  unselected row still only selects. Drag reorder stays on press-and-move.

## Open questions
- Does click-to-edit clash with drag reorder or double-click-to-open timing? Decide while building
  that line; check the drag threshold in `mouse.rs` first.

## As built
- `src/text_input.rs`: free functions over a `String` plus a byte caret, not a struct. `EditDraft`
  and `Notes` keep their own fields and call through, so no caller had to change.
  - Moves, Backspace and Delete step one grapheme (unicode-segmentation). Columns are display
    cells (unicode-width).
  - `offset_at_column` maps a click to the nearest boundary; `scroll_for`/`visible_from` give a
    stateless horizontal scroll: the caret sits on the last cell once it passes the right edge.
  - The line editor gained Delete. The notes editor's Up/Down keep a display column, not a char
    count.
- Real cursor: `ui/field.rs` draws a one-row field (scrolled for the caret) and returns the caret's
  cell; the caller calls `Frame::set_cursor_position` only while the field has the keyboard.
  ratatui hides the cursor on any frame that does not set it.
  - Wired: the list/sub-list editor (still the bottom overlay until the in-place line), the `:`
    line, the prompt bar, the parent field (only when `nav.focus == Detail`), the notes.
  - `terminal.rs` sets a steady bar on enter and `DefaultUserShape` on leave and in the panic hook.
  - Gap: the header search and Settings fields still draw `▏` (their line moves them to fields).
