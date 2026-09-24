# tui-shell

## Goal
- The c2 window chrome in a terminal: header, banners, status footer, toasts.

## Design
- Follow `apps/desktop/design-mockups/c2-prompt.html:73-221`.
- Header, left to right: live mark, workspace name (opens the `W` popup), search field, Tasks / Universal / Settings tabs, `?`.
  - Tab labels collapse to letters on narrow terminals.
- Live mark: the four logo strokes (t stem, t hook, two x strokes) fill as done/total over the root list, blank lines excluded.
  - Drawn with Unicode block/line glyphs, with a dim full mark behind.
- `g t` / `g u` / `g s` chords share the `gg` pending state in `ui/list.rs`, with a 900 ms window like the mockup.
- Toast Undo calls the daemon `Undo` RPC, not a re-toggle. The mockup re-toggles; the plan says to use the real Undo.

## As built
- Frame, top to bottom: header, banners (one row each), the screen, the footer. Toasts and popups draw over the screen.
- Header (`ui/header.rs`): mark, workspace name ▾ (W), search field (shown, inert until `tui-tasks`), tabs (letters under 80 columns), `?`.
- Live mark (`ui/mark.rs`): four box glyphs, one per stroke, bold by quarters of the root list done. Looks are for the human pass.
- `W` popup (`ui/workspace_menu.rs`, `app_workspace::open_menu`): counts come from `UniversalTasks`, which covers only loaded workspaces; the rest say "not loaded". Enter or click switches; Manage workspaces goes to Settings › Workspaces; a click outside closes.
- Keymap: `commands!` table declares `Command`, ids and bindings once. A screen scope takes the global keys, so `g g` and `g t` share their `g`.
- Banners (`ui/banner.rs`): daemon, conflict, refused edit, version, skill hint. Buttons are commands.
  - A dropped Watch no longer ends the session. The 1 s tick reconnects, bounded; past the bound, Retry starts a new round.
  - Deviation: a flagged line is read-only (edit, move, complete, delete, drag). Desktop locks its whole buffer; per line fits a line-by-line TUI.
  - Copy edit uses OSC 52 (`clipboard.rs`, hand-rolled base64). A terminal that ignores OSC 52 copies nothing, silently.
- Footer (`ui/footer.rs`): unsaved / saved (2 s) / synced / syncing N, `Ln N`, offers, refusal, hints, version. No "saving": the Apply is awaited inside the loop, so no frame could show it. The sync popup replaces the `s` strip.
- Toasts (`ui/toast.rs`): 5 s, newest lowest, max 3. `dd` and Space toast. Undo = daemon `Undo` of `ApplyResponse.applied` ops, since one change can be several ops.
- Placeholders: Universal, Settings and Help say they are not built yet.
- Not checked by eye: glyphs, colours, the daemon-down path against a killed daemon, OSC 52 in a real terminal.
