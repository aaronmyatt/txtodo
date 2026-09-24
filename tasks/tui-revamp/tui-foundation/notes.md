# tui-foundation

## Goal
- Restructure the TUI so every later screen plugs into one navigation model and one command path.

## Design
- Split files first. They are near the 400-line cap: `app.rs` 380, `state.rs` 393, `daemon.rs` 375.
  - The split commits change no behaviour.
- `Screen` (Tasks | Universal | Settings(card) | Help), `Focus` (List | Edit | Search | Prompt | Detail) and `Overlay` (WorkspaceMenu | ConflictSheet | TokenDialog | PairFlow | Confirm).
- `keymap.rs`: a static `BINDINGS` of `(id, keys, scope)`, using manifest ids.
- `commands.rs`: `run(state, Command) -> Option<Action>`.
  - The keyboard, the `:` palette and mouse hits all go through `Command`.
  - `input.rs` shrinks to a key → binding lookup.
- `theme.rs`, ink/paper:
  - Default fg/bg (`Color::Reset`); `REVERSED` for inverse tiles and selection.
  - Token colours from `apps/desktop/src/app.css:56-63` and `:96-103`.
  - RGB when `COLORTERM=truecolor`, otherwise the nearest ANSI 16.
- Terminal:
  - `EnableMouseCapture`.
  - `PushKeyboardEnhancementFlags` where the terminal supports it, so Ctrl-Enter and Ctrl-Shift-Space can be told apart.
  - Ctrl-C quits.
  - Everything is restored on exit and on panic.
  - Ref: https://docs.rs/crossterm/latest/crossterm/event/
- Workspace switch mirrors desktop `switch_workspace`: swap the selector, re-watch, re-baseline.

## As built
