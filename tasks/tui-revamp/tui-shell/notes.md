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
