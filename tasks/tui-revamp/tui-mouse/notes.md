# tui-mouse

## Goal
- Everything clickable on desktop is clickable in the TUI, plus drag reorder.

## Design
- `hit.rs`: each `draw` records `Vec<(Rect, Target)>` into state.
- `mouse.rs`: `on_event(state, MouseEvent) -> Option<Command>` looks the event up in the hit map.
  - It is pure, so tests run over a fixed map with no terminal.
- crossterm has no double-click event.
  - Two `Down(Left)` events on the same cell within 400 ms count as a double-click.
- Hover uses the motion events that `EnableMouseCapture` turns on.
- Drag reorder reuses `input.rs` `move_selected_*` / `task_ref_at`, so blank lines are never addressed.
  - Desktop has no drag. That is an `@parity` line.
- Ref: https://docs.rs/crossterm/latest/crossterm/event/struct.MouseEvent.html

## As built
- `hit.rs`: `HitMap` of `(Rect, Target)` in drawing order; `Target::{Row, Command, Inert}`. `ui::screen::draw` returns it; `app_loop` stores it in `AppState.hits` and copies the list's scroll into `AppState.scroll`.
  - Overlays record `Inert` over what they cover, so a click never reaches a row under them.
- `mouse.rs`: `Mouse::on_event -> Option<Action>` (a drop is a mutation, so not `Command`). `Input::on_mouse` wraps it; a press clears a half-typed chord.
  - Acts in list mode only. While editing, in the `:` line or with a sheet up, it does nothing.
  - Double-click edits the row for now. `tui-detail` re-points it to open detail.
  - Wheel: 3 rows a notch, then the cursor is pulled into view (else drawing scrolls back to it).
  - Hover: desktop's `--color-hover-overlay` over its paper, as RGB; underline in 16 colours.
  - Drag: `commands::move_row` drops the row where it is released; a blank drop row counts as the task above it; the Add-a-line row is the end. `tests/mouse_drag.rs` checks the file on disk.
- Not done here: targets on screens that don't exist yet. Each screen's backlog now has a Mouse line.
- Human check: click, double-click, wheel, hover and drag in iTerm2 or kitty, and Terminal.app.
