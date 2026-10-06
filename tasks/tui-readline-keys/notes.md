# tui-readline-keys

## Goal
- Shell-style cursor motions in every TUI text field (asked 2026-10-06):
  - `Ctrl-A` start of line, `Ctrl-E` end of line.
  - `Alt-B` back one word, `Alt-F` forward one word.
- Fields: the line and sub-line editor, the parent field, the prompt bar, the notes, the header
  search, the `:` line, the Settings filter and field.

## Decisions (2026-10-06, agent; the human delegated these calls)
- **Words are readline words: runs of letters and digits** (UAX #29 word bounds, so CJK and accents
  count). `Alt-F` goes to the end of the current or next word, `Alt-B` to the start of the current
  or previous one. So `+home`, `due:2026-10-09` and `@phone` take more than one hop.
  - Rejected: whitespace words (vim `W`). One hop per todo.txt token is handy, but it is not what
    `Alt-B`/`Alt-F` do in a shell, and the muscle memory is the point.
- **`Alt-Left`/`Alt-Right` do the same as `Alt-B`/`Alt-F`.** On macOS, Option-arrow sends them
  in most terminals even with Option-as-Meta off.
- **In the notes, `Ctrl-A`/`Ctrl-E` go to the start/end of the logical line**, the same as
  Home/End there today, not of the wrapped row.
- **Word motion wins over the prompt bar's `Alt-b` chip.** Today `Alt-b` applies the `(B)` chip.
  That chip's key becomes `Alt-B` (shift). The click still works.
  - Cost: `(A)`/`(C)` stay `Alt-a`/`Alt-c`, so the three priority chips are no longer one
    pattern. Rejected: moving all three to `Alt-1/2/3`, which changes two keys nobody asked about.
- **The keys go in `keymap::BINDINGS` (scope `edit`) with manifest rows**, so Help lists them and
  `tests/it/parity.rs` holds them. The command acts on whichever text field has the keyboard,
  through `text_input`. The manifest's `edit` scope is read as "any text field".
- **Desktop is `differs`, not `planned`:** a browser text field uses the platform's word keys
  (Option-Left/Right on macOS, Ctrl-Left/Right elsewhere). macOS already gives it `Ctrl-A`/`Ctrl-E`.
  So no desktop `@parity` line.

## Known limits
- `Alt` reaches the TUI only if the terminal sends Option as Meta (Terminal.app: "Use Option as
  Meta key"; iTerm2: Option key "Esc+"; Ghostty: `macos-option-as-alt`). Otherwise Option-B types
  `∫`, and no app can tell. Help says so.
- `Ctrl-A` is a common tmux prefix: inside such a tmux, tmux takes it first.

## Not filed (optional, ask)
- The rest of the readline set: `Ctrl-W`/`Alt-Backspace` delete a word back, `Alt-D` a word
  forward, `Ctrl-U`/`Ctrl-K` kill to start/end, `Ctrl-B`/`Ctrl-F`/`Ctrl-D` by character.
