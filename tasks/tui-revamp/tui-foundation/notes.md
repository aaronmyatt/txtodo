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
- Files: `state_nav.rs` (Screen, SettingsCard, Focus, Overlay, Nav), `state_types.rs`, `app_loop.rs`, `daemon_{workspace,notes,history,devices,tokens,activity}.rs`, `keymap.rs`, `commands.rs`, `theme.rs`, `terminal.rs`.
- Keys: key → `keymap::key_name` → `Chords::resolve` per scope → `Command` → `commands::run` → `Action` → `app::perform`. The `:` line runs any manifest id; `:w <name>` switches workspace; `:q` and Ctrl-c quit.
  - Ctrl-c is checked before any scope, so it quits from the editor and the `:` line too.
  - `Nav` exists but nothing reads it yet: `input.rs` still picks scope from `conflicts_open` / `offers.open`. The screens that need it (shell, Settings) wire it.
- Theme: token colours only, ink/paper stay the terminal's. 16-colour table picked by hue: nearest-by-distance merged five token pairs.
  - `ThemeMode` is always System (COLORFGBG) until Settings › Appearance stores a choice.
- Terminal: mouse capture on; kitty flags (disambiguate only) when the terminal answers the query. `leave()` and a chained panic hook undo both.
  - Not yet seen in a real terminal. Human check: run `txtodo-tui`, press Ctrl-c, and the shell should echo keys and not print mouse junk on click.
- Workspace switch: `app_workspace::switch_workspace` swaps the selector, re-watches (`state.rewatch`), re-baselines.
- Found on the way: a Watch change for the open document never re-fetched it, so other clients' edits showed only after a reconnect. `app::follow_change` re-fetches now; `tests/external_edit.rs` covers it.
- `show_id` dropped: no key toggled it, and desktop always hides ids.
