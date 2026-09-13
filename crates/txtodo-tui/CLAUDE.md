# txtodo-tui

## Purpose
The ratatui client: vim keys, live sync indicator, conflict review (plan M10, design §7).
Built 2026-09-13. **Not yet wired to real users**: `main()` runs a real event loop against a real
daemon, but `SyncStatus` (the `s` indicator's live data) doesn't exist on the wire yet — see
Invariants below for the exact gap and why.

## Public interface
- `paint::{paint_line, token_style}` — `TokenKind` -> styled `ratatui::text::Line`/`Span`
  (design §3.1 semantic colours), completed-line mute+strike, `id:` hidden unless shown.
- `state::{AppState, LineState, EditDraft, EditTarget, ConflictItem, PeerStatus, SyncSnapshot,
  Resolution}` — everything the UI renders from; `AppState::fixture()` for tests/fixtures,
  `AppState::from_document` from real bytes. Navigation (`move_down`/`up`/`first`/`last`),
  toggles (`show_id`/`sync_visible`/`conflicts_open`), and the `:` command line
  (`start_command`/`cancel_command`/`run_command`) all live here.
- `ui::list::{rows, list_widget, ListInput}` — the line list + `j`/`k`/`gg`/`G` + the trailing
  Add-a-line row. `ui::edit::{start, on_key, commit, cancel, OpenKey}` — the `i`/`a`/`A` single-
  line editor, mapping a finished draft to an `Add`/`Edit` `Mutation`. `ui::conflicts::{on_key,
  move_down, move_up, resolve_request}` — the `r` pane, resolving through the daemon's own
  `ResolveConflict` RPC (mine/theirs/merged) rather than a hand-rolled `Apply`. `ui::sync::{render,
  widget}` — the `s` indicator's pure rendering of a `SyncSnapshot`. `ui::screen::draw` — composes
  all of the above into one `ratatui::Frame`.
- `daemon::{Daemon, DaemonError, socket_path, MAX_RECONNECT_ATTEMPTS}` — the gRPC bridge to
  `txtodod` over the ADR 0010 unix socket (mirrors `apps/desktop/src-tauri/src/daemon.rs`'s
  `DaemonClient`): `connect`/`wait_until_ready`/`get_file`/`watch`/`apply`/`list_conflicts`/
  `resolve`. **No `sync_status` method** — see Invariants.
- `input::Input` — pure vim-key dispatch (`AppState` + one `KeyEvent` -> an optional
  `action::Action`), daemon-free and fully unit tested; `app::{run, perform, reconnect_watch,
  main}` is the async glue that actually sends an `Action` to a real `Daemon` and drives the
  terminal.
- Tests: 43 unit tests (`cargo test -p txtodo-tui --lib`) covering every module above against
  `AppState::fixture()`/hand-built drafts, no daemon needed. 7 integration tests
  (`tests/roundtrip.rs`, `tests/external_edit.rs`) spawn a real `txtodod` (via `tests/support`,
  which locates/builds `target/debug/txtodod` by path since `CARGO_BIN_EXE_txtodod` is only set
  for a binary in its *own* package) and drive `Daemon`/`app::perform`/`app::reconnect_watch`
  directly: `dd`/`Space`/`i`+save round-trip through `Apply` and are visible on a real `Watch`
  stream; an external file write appears on `Watch` without a manual refresh; a dropped `Watch`
  reconnects (bounded) and re-baselines; reconnecting past the bound with no daemon at all fails
  rather than looping forever.

## Invariants
- Thin client: talks to the daemon, never parses the file — every byte painted comes from
  `GetFile`/`Watch`; the only file-shaped work done in-process is `txtodo_core::tokenize` for
  colouring (design §7 explicitly allows this: "identical token boundaries everywhere").
- Every buffer change is an `Apply`; this crate never writes `todo.txt` itself.
- **Known gap, not silently dropped:** design's `s` indicator calls for a `rpc SyncStatus`
  (`SyncStatusResponse{ peers, pending_ops }`) that does not exist in `crates/txtodo-proto` yet,
  and a daemon-side handler that doesn't exist in `crates/txtodo-daemon` yet. Adding both needs
  edits outside this crate — this session's edit fence (`.claude/hooks/fence.sh`, "one slice per
  session") allows exactly one crate per session, and `txtodo-tui` was it. `ui/sync.rs` is fully
  built and tested against a `SyncSnapshot` fixture (`state.rs`'s own UI-local mirror of the
  future wire shape, deliberately *not* the generated `pb` type, so this crate needed no proto
  change to build); `AppState.sync` is simply never populated by `app.rs` today. Follow-up task:
  add `SyncStatus` (+ the `Tokenizer`/`Complete` messages `tasks/tui/notes.md` bundles with it,
  for `editor-plugins`) to the proto in a `txtodo-proto`-slice session, a handler in a
  `txtodo-daemon`-slice session, then wire `Daemon::sync_status` + a 1 s tick in `app.rs` here.
  The same fence blocked seeding a real `needs_review` conflict flag for an integration test
  (`crates/txtodo-daemon/tests/grpc.rs::raise_flag` needs `txtodo_store` directly, out of
  `allowedDeps`) and a real two-daemon LAN-sync convergence test for the `s` indicator (needs the
  same missing `SyncStatus` RPC) — both `ui::conflicts`/`ui::sync`'s own logic is unit tested
  against fixtures instead.
- May depend only on: txtodo-core, txtodo-proto (external: ratatui, crossterm, tonic, tokio,
  hyper-util, tower, jiff — same socket-dial set `txtodo-cli` already carries, `cargo deny check`
  clean as of 2026-09-13, human sign-off still outstanding).
