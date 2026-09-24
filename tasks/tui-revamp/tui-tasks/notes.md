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
- Rows (`ui/row.rs`): gutter, done dim + strike, text past 100 chars underlined (replaces the `[100+]` tag), `ref:` badge `n/m` or `¶`.
  - Badges come from `ListFiles`' tree (`app_refs.rs`) on start and on the 1 s tick: a sub-list change comes on no Watch the TUI holds.
  - The tree lists `notes.md` only once the daemon has opened it, so a ref dir without tasks shows `¶`.
  - The underline counts drawn chars; a mid-line `id:` tag shifts it by one.
- Sub-toolbar (`ui/subbar.rs`): path, `t` / `x` counts, lines; while searching, "k of N lines match".
- List mode: `x` = Space, `Alt-Up/Down` = K/J, `u` = the daemon's Undo of this session's newest change, all its ops (`Shell.changes`). A toast's Undo uses the same stack, so it only works while its change is the newest.
  - Enter / Ctrl-Enter wait for `tui-detail`.
- Search (`search.rs`): `/` or a click focuses; Enter / Shift-Enter step through hits; Esc clears, then leaves. Rows mark hits, fade misses; the header shows `i/N`.
  - Term positions are found in the TUI; core has no range API. A candidate for core when desktop needs it.
  - Shift-Enter needs the kitty keyboard protocol; legacy terminals send Enter.
- Suggestions (`search_suggest.rs`, `ui/search_panel.rs`): top contexts/projects, `(A)`–`(C)`, `is:`, `due:`, recents (session only). Click toggles; Tab completes or adds.
- Conflict sheet (`ui/conflict_sheet.rs`): modal, k of N, mine / theirs / merged preview as a char diff, buttons; closes after the last flag. The diff-to-runs step copies `txtodo-ffi`'s `diff_view`.
- Tests: `tests/list_mode.rs`, `tests/ref_badges.rs`, plus unit tests per module.
