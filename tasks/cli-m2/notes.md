# M2 — `txtodo` CLI, direct-file mode

Plan M2. Nine (A) lines in ./todo.txt share this note. Each lands as its own ≤300-line commit:
deps+config+`env` · store+`add`/`addm` · listings · `--json` · `do`/`archive`/`pri`/`depri`/`del` ·
`append`/`prepend`/`replace`/`move`/`deduplicate`/`report` · `fmt`/`lint` · vendored todo.sh (alone) ·
parity harness · CRLF/no-newline tests.

## Shape
- `Env { vars, cwd }` is injected (never `std::env` below `main`); `Paths` = dir, todo, done, report, config.
- Precedence: `--dir` > `TXTODO_TODO_DIR` > config `todo_dir` > cwd. Config: `$TXTODO_CONFIG` >
  `$XDG_CONFIG_HOME/txtodo/config.toml` > `%APPDATA%` > `~/.config` (plan §1 decision 10).
- Line numbers are 1-based over every line, blanks included (todo.sh `sed "$item!d"`); a blank or
  out-of-range item is `TODO: No task N.` exactly like todo.sh.
- Writes: read bytes → `core::parse_file` → mutate → `File::to_bytes` → temp file beside it → rename.
  Endings, BOM, trailing newline round-trip (design §2.2 rule 7). Appending to a file whose last line
  lacks a newline first terminates it with the file's dominant ending (todo.sh `fixMissingEndOfLine`).

## Parity decisions (todo.sh v2.12.0, vendored for the harness)
- `add` = todo.sh `-t`: uppercase `(a)`, CR/LF → space, stamp today after the priority, then ` id:ULID`
  unless `--no-id` / `id_tags = false`. The harness always runs `txtodo --no-id`.
- `do` completes via `core::Edit::complete` (writes `pri:P`, design §2.2 rule 3) then auto-archives
  like todo.sh. todo.sh drops the priority instead, so parity scenarios complete unprioritised tasks;
  a txtodo-only test pins `pri:`.
- `archive` deletes every blank line first, then moves `x ` lines to done.txt (todo.sh order).
- `del`/`move`/`deduplicate` leave a blank line (TODOTXT_PRESERVE_LINE_NUMBERS=1 default).
- `append`/`prepend`/`replace`/`del TERM` are raw-text operations in todo.sh (sentence delimiters,
  priority+date prefix kept); they are reproduced on the raw line, not via `Edit`, so bytes match.
