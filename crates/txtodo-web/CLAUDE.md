# txtodo-web

## Purpose
The browser peer's host (ADR 0042, task `web-pwa`, 2026-10-17): what `txtodod` is to the desktop
app, run in a dedicated Web Worker. Lists are `txtodo_engine` documents over `txtodo-store`'s op
log, SQLite in OPFS there. Local only so far: sync, keys and pairing are the web-pwa "Sync from the
browser" line.

## Public interface
- `host::Host::open(Store)`: this device's id (meta `web/device`, minted once) and the known lists
  (meta `web/files`, `todo.txt` the first time). `call(cmd, &Value) -> Result<Value, WebError>`
  answers `list_files`, `get_file`, `apply`, `history`, `list_duplicates`, `watch` (a no-op) with
  the JSON `apps/desktop/src-tauri/src/dto.rs` sends; `undo_change` is refused for now and any
  other command is `UnknownCommand` (the Worker shim answers app, window and device commands).
  `take_changes()` hands over the `ChangeDto`s since the last call, at most
  `MAX_PENDING_CHANGES`.
- `doc::Doc`: one list. `open` replays the list's ops (`apply_leniently`) into an empty sidecar
  `DocState`; `apply(store, mutations, &By)` is the daemon's apply path in small: one tick past the
  peer-stamp floor (ADR 0040), `mutation_ops`, one `commit_change`; a lone `Replace` reconciles and
  settles through the engine (`reconcile_replay`) and commits the ops as logged.
- `mutation_dto`: the frontend's `{ "kind": ... }` mutations into the engine's, with the daemon's
  wire fixes (`fix_case`, `date_done`, `stamp_created`).
- `wasm` (wasm32 only): `WebHost.open(db)` installs `sqlite-wasm-vfs`'s OPFS pool as SQLite's
  default VFS, then `Store::open` unchanged; `call(cmd, argsJson)`, `takeChanges()`, `device()`.
- Tests: `host_tests.rs`, natively over a real SQLite file. In a browser it was checked by hand on
  2026-10-17 (a Worker, three loads and a fresh context; recipe in `tasks/web-pwa/notes.md`).

## Invariants
- The op log is the truth: no file stands behind a list here, and reopening replays the ops.
  A `Replace` whose ops cannot render the text exactly is refused (the daemon adopts the file
  then; there is no file to adopt).
- Refused for now, with a message: a cross-file `Move`, `undo_change`, notes, a list past
  `MAX_OPS_PER_DOC` ops, more than `MAX_LISTS` lists.
- Building for wasm32 needs a clang that targets it (ring, SQLite): Homebrew `llvm@21` via
  `CC_wasm32_unknown_unknown`/`AR_wasm32_unknown_unknown`, apt clang in CI's `wasm` job.
- May depend only on: txtodo-core, txtodo-model, txtodo-store, txtodo-engine (external: serde,
  serde_json, getrandom, tracing; on wasm32 wasm-bindgen, wasm-bindgen-futures, js-sys,
  sqlite-wasm-rs, sqlite-wasm-vfs).
