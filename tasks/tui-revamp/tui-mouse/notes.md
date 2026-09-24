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
