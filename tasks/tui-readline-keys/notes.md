# tui-readline-keys

## Goal
- Shell-style (readline, emacs mode) keys in every TUI text field (asked 2026-10-06):
  - Motions: `Ctrl-A`/`Ctrl-E` line start/end, `Alt-B`/`Alt-F` word back/forward, `Ctrl-B`/`Ctrl-F`
    one character.
  - Kill and yank: `Ctrl-W`, `Alt-Backspace` (word back), `Alt-D` (word forward), `Ctrl-U`/`Ctrl-K`
    (to line start/end), `Ctrl-Y` (yank the last kill).
  - Edits: `Ctrl-D`/`Ctrl-H` delete forward/back, `Ctrl-T`/`Alt-T` transpose characters/words,
    `Alt-U`/`Alt-L`/`Alt-C` upper/lower/capitalize a word, `Ctrl-_` undo.
- Fields: the line and sub-line editor, the parent field, the prompt bar, the notes, the header
  search, the `:` line, the Settings filter and field.

## Decisions (2026-10-06, agent; the human delegated these calls)
- **Words are readline words: runs of letters and digits**, counted by grapheme (an accent stays
  with its letter; a run of ideographs is one word; an emoji is not a word). `Alt-F` goes to the end of the current or next word, `Alt-B` to the start of the current
  or previous one. So `+home`, `due:2026-10-09` and `@phone` take more than one hop.
  - Rejected: whitespace words (vim `W`). One hop per todo.txt token is handy, but it is not what
    `Alt-B`/`Alt-F` do in a shell, and the muscle memory is the point.
- **`Alt-Left`/`Alt-Right` do the same as `Alt-B`/`Alt-F`.** On macOS, Option-arrow sends them
  in most terminals even with Option-as-Meta off.
- **In the notes, `Ctrl-A`/`Ctrl-E` go to the start/end of the logical line**, the same as
  Home/End there today, not of the wrapped row.
- **Readline wins over the prompt bar's Alt chips; every chip key moves to Alt-Shift** (`Alt-A`,
  `Alt-B`, … `Alt-R`). Today `Alt-a b c x p o d t r` apply chips, and readline wants `Alt-b`,
  `Alt-c`, `Alt-d`, `Alt-t` (and `Alt-f`, `Alt-u`, `Alt-l`). One rule for all nine chips beats
  moving four of them. The clicks still work.
  - Cost: every chip key needs Shift now.
  - Rejected: moving only the four that clash (no pattern left), and `Alt-1…9` (digits say
    nothing about the chip).
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

- **One kill slot, shared by every field**, not a ring: `Ctrl-Y` pastes the last kill. Kills
  that follow each other append to it, as in readline. It is not the system clipboard (the TUI's
  OSC 52 copy stays separate).
  - Rejected: a full kill ring with `Alt-Y` (rotate): little use in one-line fields.
- **Undo is per field and per edit session**: `Ctrl-_` steps back through this field's edits since
  it took the keyboard; Esc still discards the whole draft. The list's `u` (daemon Undo) is
  unrelated.
- **`Ctrl-D` on an empty field does nothing** (no end-of-file); `Ctrl-H` is Backspace, which many
  terminals already send.
- **In the notes, `Ctrl-K` at the end of a line deletes the line break** (emacs), so repeated
  `Ctrl-K` empties a block; `Ctrl-U` kills back to the start of the logical line.
- Not included: `Alt-R` (revert line, same as Esc here), `Alt-.` (last argument: no history).

## As built
- `src/readline.rs` (2026-10-06): one `Op` per key and `apply(op, text, caret, kill)`, pure, over
  the `String` + byte caret every field keeps. Words are runs of graphemes holding a letter or
  digit (first built on UAX #29 word bounds, which join `list.last` and `don't` into one word, so
  dropped for readline's rule); `Ctrl-W` alone goes back to a space (readline's `unix-word-rubout`). `Kill` is
  the one shared slot: kills in a row join (backward in front, forward behind), any other op starts
  it over. `Ctrl-K` at a line's end takes the line break. `Ctrl-T`/`Alt-T` follow readline: the
  caret ends after the swapped pair. `Undo` keeps snapshots; a run of typed characters is one step.
- Keys and wiring: twenty `edit.*` commands in `keymap/table.rs` (scope `edit`), with manifest
  rows (desktop `differs`: CodeMirror has the Ctrl ones on macOS only, the rest are platform keys).
  `Input::on_key_at` hands a readline key to `fields::run` first whenever a text field has the
  keyboard (`fields::focused`, same order as the key routing), so no field's own handler sees it;
  in list mode the keys do nothing. `fields.rs` reaches each field's text and caret (byte or
  `TailCaret`), keeps the shared `Kill` and the per-field `Undo` in `Shell`, records an undo step
  after any key that changed the field (a typing run is one), and starts the steps over when the
  keyboard moves to another field.
  - `Ctrl-_` arrives as Ctrl-7 from legacy terminals (byte 0x1F); `keymap::key_name` names it
    `Ctrl-_`. `Ctrl-/` is bound too.
  - Edits made here mark the notes typed (autosave) and re-pick the Settings card for the filter.
- Prompt chips: all nine take `Alt-Shift` and their letter (`prompt::CHIPS` keys are capitals);
  Help's chip list and the `prompt.chip` deviation say so. An input test shows `Alt-B` applying
  `(B)` while `Alt-b` moves a word.
- Help: the `edit` heading is "Editing text (every field)", with the Option-as-Meta note under it.
- Tests: `readline_tests.rs` (every op, kill joining, undo grouping), `fields_tests.rs` (line
  editor, prompt, search, `:` line, Settings filter, notes, shared kill slot, undo with Ctrl-7,
  list mode untouched). Not covered by a test: the parent field (same `Slot::Byte` path as the
  line editor).
