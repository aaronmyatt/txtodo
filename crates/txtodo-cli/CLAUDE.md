# txtodo-cli

## Purpose
The `txtodo` binary: todo.sh-compatible commands over todo.txt in direct-file mode. Plan M2.
Daemon mode (socket, history, sync) is M3 and not here yet.

## Public interface
- Commands and aliases exactly as todo.sh 2.14: `add`/`a`, `addm`, `list`/`ls`, `listall`/`lsa`,
  `listpri`/`lsp`, `listproj`/`lsprj`, `listcon`/`lsc`, `listfile`/`lf`, `do`, `pri`/`p`,
  `depri`/`dp`, `append`/`app`, `prepend`/`prep`, `replace`, `del`/`rm`, `move`/`mv`, `archive`,
  `deduplicate`, `report`; plus `fmt`, `lint`, `env`.
- Line numbers are the ids: 1-based over every line, blanks included.
- Global flags: `--dir DIR`, `--json` (listings and `env`, `lint`), `--no-id`, `-A/--no-archive`.
- Config `config.toml` (`todo_dir`, `id_tags`, `url_schemes`) at `$TXTODO_CONFIG`, else
  `$XDG_CONFIG_HOME|%APPDATA%|~/.config` + `txtodo/config.toml`; env `TXTODO_TODO_DIR`.
  Precedence: `--dir` > env > config > cwd.
- Module map: `config` (env + paths), `store` (read, atomic write, append), `clock` (today, ULID,
  timestamp), `json`, `commands::{add, list, edit, archive, text, fileops, hygiene}`.

## Invariants
- Writes are atomic: temp file beside the target, fsync, rename. Untouched lines round-trip
  byte-for-byte; BOM, per-line endings and the trailing newline are kept (design §2.2 rule 7).
  Only `fmt` canonicalises; only `archive` (and `do` through it) drops blank lines, as todo.sh does.
- todo.sh parity is a test, not a hope: `tests/todosh_parity.rs` runs every scenario through
  `tests/vendor/todo.sh` (v2.14.0, verbatim, executable) and asserts byte-identical files. Add a
  scenario when touching a command. Known, deliberate difference: `do` keeps the priority as `pri:`.
- `add` stamps today's local date after the priority (todo.sh `-t`) and appends `id:<ULID>` unless
  `--no-id` or `id_tags = false`. `Env`, today and ULIDs enter at `main`; command logic takes values.
- May depend only on: txtodo-core, txtodo-proto.
