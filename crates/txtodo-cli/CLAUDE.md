# txtodo-cli

## Purpose
The `txtodo` binary: todo.sh-compatible commands over todo.txt, in direct-file mode (M2) or through
txtodod when `<dir>/.txtodo/txtodod.sock` exists (M3, as built 2026-09-12).

## Public interface
- Commands and aliases exactly as todo.sh 2.14: `add`/`a`, `addm`, `list`/`ls`, `listall`/`lsa`,
  `listpri`/`lsp`, `listproj`/`lsprj`, `listcon`/`lsc`, `listfile`/`lf`, `do`, `pri`/`p`,
  `depri`/`dp`, `append`/`app`, `prepend`/`prep`, `replace`, `del`/`rm`, `move`/`mv`, `archive`,
  `deduplicate`, `report`; plus `fmt`, `lint`, `env`.
- Daemon-only: `log [--file F] [-n N]`, `blame ITEM#`, `undo [--steps N]`,
  `checkout YYYY-MM-DDTHH:MM[:SS] [--stdout] [--file F]` (fail with the fix in direct mode);
  `doctor [--verbose]` (six fixed checks plus one row per known sync peer, exit 1 on any FAIL);
  `daemon install|start|stop|status [--force]`; `device list|remove <id> [--yes]` (plan M4
  tasks/sync-device-remove — removal confirms by making the human type the id back unless `--yes`).
- Line numbers are the ids: 1-based over every line, blanks included.
- Global flags: `--dir DIR`, `--json`, `--no-id`, `-A/--no-archive`, `--no-daemon`.
- Config `config.toml` (`todo_dir`, `id_tags`, `identity_mode`, `key_store`, `url_schemes`) at
  `$TXTODO_CONFIG`, else `$XDG_CONFIG_HOME|%APPDATA%|~/.config` + `txtodo/config.toml`; env
  `TXTODO_TODO_DIR`. Precedence: `--dir` > env > config > cwd. `identity_mode =
  "tagged"|"sidecar"` (docs/questions.md Q2) is the name to reach for going forward; `id_tags`
  still works alone for an existing config, but an unset config is `Sidecar` now, not `Tagged`
  (plan §1 decision 9, reversed). `key_store = "auto"|"os"|"file"` (plan M4 `sync-keystore`,
  `Config::key_store_mode()`) mirrors the daemon's own `--key-store` flag by name; like
  `identity_mode`, nothing in this crate threads it through the service install templates yet
  (a pre-existing gap for that flag, not new here) — set it directly on `txtodod --key-store`
  for now. `txtodo env` reports the effective value.
- Module map: `config`, `store` (read, atomic write), `clock`, `json`, `error` (CliError),
  `client` (gRPC over the socket, own current-thread runtime), `daemon_mode` (scratch-copy
  adapter, `plan_mutations`), `commands::{add, list, edit, archive, text, fileops, hygiene,
  history, doctor, device, env, service}`; `main` = `dispatch` (direct) and `dispatch_daemon`.

## Invariants
- Writes are atomic: temp file beside the target, fsync, rename. Untouched lines round-trip
  byte-for-byte; BOM, per-line endings and the trailing newline are kept (design §2.2 rule 7).
- Daemon mode never touches a synced file directly except as the documented fallback: a command
  runs against a scratch copy of the daemon's bytes and its diff becomes Apply mutations (edits,
  deletes bottom-up, appends). Inexpressible diffs (blank removal by `archive`, mid-file inserts,
  moves) write the scratch bytes to the real file; the daemon reconciles them as an External edit.
- Mode selection: socket missing or `--no-daemon` → direct; socket present → connect; present but
  refused → error with the fix (`txtodo doctor`, `--no-daemon`). Never a silent fallback.
- todo.sh parity is a test: `tests/todosh_parity.rs` (direct mode). `tests/daemon_mode.rs` spawns a
  real txtodod (built on demand with cargo — this crate may not depend on the daemon crate).
- `add` always stamps today's local date; it stamps `id:<ULID>` too only when `Config::id_tags()`
  says so (`identity_mode` `Tagged`, or `--no-id` never overrides an explicit request to skip it) —
  unset config means `Sidecar`, so a fresh workspace gets no `id:` tag from either mode (daemon
  mode reuses this same direct-mode `add` against its scratch copy, so the same default applies
  there too; the daemon mints its own id regardless of what the text does or doesn't carry).
  `Env`, today and ULIDs enter at `main`; command logic takes values.
- May depend only on: txtodo-core, txtodo-proto.
