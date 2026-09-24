# txtodo-tui

## Purpose
The ratatui client: vim keys, live sync indicator, conflict review (plan M10, design §7).
Built 2026-09-13; the sync indicator's `SyncStatus` RPC (proto, daemon handler, and this crate's
own `app.rs`/`daemon.rs` wiring) landed 2026-09-18 across three slice-fenced sessions — see
Invariants below for the one real gap that RPC surfaced but did not fix.

## Public interface
- `paint::{paint_line, token_style}` — `TokenKind` -> styled `ratatui::text::Line`/`Span`
  (design §3.1 semantic colours), completed-line mute+strike, `id:` hidden unless shown.
- `state::{AppState, LineState, EditDraft, EditTarget, ConflictItem, PeerStatus, SyncSnapshot,
  Resolution}` — everything the UI renders from; `AppState::fixture()` for tests/fixtures,
  `AppState::from_document` from real bytes. Navigation (`move_down`/`up`/`first`/`last`),
  toggles (`sync_visible`/`conflicts_open`), and the `:` command line
  (`start_command`/`cancel_command`/`run_command`) all live here.
- `ui::list::{rows, list_widget, ListInput}` — the line list + `j`/`k`/`gg`/`G` + the trailing
  Add-a-line row. `ui::edit::{start, on_key, commit, cancel, OpenKey}` — the `i`/`a`/`A` single-
  line editor, mapping a finished draft to an `Add`/`Edit` `Mutation`. `ui::conflicts::{on_key,
  move_down, move_up, resolve_request}` — the `r` pane, resolving through the daemon's own
  `ResolveConflict` RPC (mine/theirs/merged) rather than a hand-rolled `Apply`. `ui::sync::{render,
  widget}` — the `s` indicator's pure rendering of a `SyncSnapshot`. `ui::offers::{on_key, draw,
  accept_request, decline_request}` + `state_offers::{OfferItem, OffersPane}` + `app_offers` — the
  `o` workspace-offers pane (task `workspace-offer-cli`): `a` accepts at once (the daemon mirrors
  into its own folder, task `remote-workspace-mirror`, and on its own too), `d` declines; the list
  refreshes on the same 1 s tick as `s`, and the status line counts pending offers.
  `app_workspace::pick_workspace` labels the status line `default workspace` or, inside a mirror,
  `remote workspace`. `ui::screen::draw` —
  composes all of the above into one `ratatui::Frame`.
- `daemon::{Daemon, DaemonError, socket_path, MAX_RECONNECT_ATTEMPTS}` — the gRPC bridge to
  `txtodod` over the ADR 0010 unix socket (mirrors `apps/desktop/src-tauri/src/daemon.rs`'s
  `DaemonClient`): `connect`/`wait_until_ready`/`get_file`/`watch`/`apply`/`list_conflicts`/
  `resolve`/`sync_status`.
- `input::Input` — pure vim-key dispatch (`AppState` + one `KeyEvent` -> an optional
  `action::Action`), daemon-free and fully unit tested (`input_tests.rs`, split out for the
  400-line file cap). **root todo 9**: `J`/`K` build a `MoveBefore`/`MoveToEnd` mutation that
  swaps the selected line with its neighbor, then advance/retreat `state.cursor` so it keeps
  tracking the moved line once the daemon's repaint lands — unlike desktop's drag reorder (task
  `desktop-reorder-propagates`), which sends a whole-document `Replace` because its buffer has no
  ids under Sidecar; the TUI already renders one `LineState` per line with `line_number`/`task_id`,
  so a same-file `MoveBefore` per line addresses both ends of the swap directly. `app::{run,
  perform, reconnect_watch, main}` is the async glue that actually sends an `Action` to a real
  `Daemon` and drives the terminal. `app.rs` also polls `Daemon::sync_status` on a 1 s tick
  (`SYNC_STATUS_INTERVAL`),
  mapping the response into `AppState.sync` via `to_sync_snapshot` — best-effort: a failed poll
  leaves the previous snapshot in place rather than erroring the event loop. **`tasks/
  daemon-always-available`**: `app::async_main` now calls `txtodo_daemon_launch::ensure_daemon`
  right before `Daemon::wait_until_ready`, so a missing `txtodod` is spawned rather than only
  ever erroring — honoring `TXTODO_NO_AUTOSTART=1` as an opt-out. **`tasks/
  tui-global-socket-migration`**: that daemon is the one device-global `txtodod`
  `txtodo`/`txtodo-mcp`/`apps/desktop` all dial too (`LaunchConfig::new`, no `.with_dir`), not a
  per-workspace bridge daemon of its own — `Daemon` carries an `Option<pb::WorkspaceSelector>`
  (`daemon.rs::workspace_selector`), cloned onto every request, so a device-global daemon with
  several open workspaces still routes each RPC to the right one. `socket_path`/`with_dir` remain
  for the legacy per-workspace bridge daemon this crate's own test harness still spawns (hermetic,
  one workspace per daemon, unambiguous with `selector: None`). The `ensure_daemon` result is deliberately
  ignored; `wait_until_ready`'s own error is still the one message a user sees.
- Tests: 61 unit tests (`cargo test -p txtodo-tui --lib`) covering every module above against
  `AppState::fixture()`/hand-built drafts, no daemon needed. Integration tests
  (`tests/roundtrip.rs`, `tests/external_edit.rs`, `tests/sync_status.rs`) spawn a real `txtodod`
  (via `tests/support`, which locates/builds `target/debug/txtodod` by path since
  `CARGO_BIN_EXE_txtodod` is only set for a binary in its *own* package) and drive
  `Daemon`/`app::perform`/`app::reconnect_watch` directly: `dd`/`Space`/`i`+save round-trip
  through `Apply` and are visible on a real `Watch` stream; `J`/`K` reorder a line against a real
  daemon and land on disk in the new order; an external file write appears on `Watch` without a
  manual refresh; a dropped `Watch` reconnects (bounded) and re-baselines; reconnecting past the
  bound with no daemon at all fails rather than looping forever; `sync_status` round-trips against
  a real daemon with no peers (the two-loopback-daemon, real-peer version of this test is still
  open — `tasks/tui/todo.txt`).

## Parity
`specs/client-parity.toml` is the one list of user-facing actions and where this client and the
desktop app each stand on them (ADR 0031, `tasks/tui-revamp/parity-manifest`).
- Change the manifest in the same commit as any key or user-facing action added, changed or
  dropped here. A manifest `id` is also the `:` palette command.
- The client that lags gets an `@parity` backlog line naming the action id: desktop's in
  `tasks/desktop-ui-revamp/todo.txt`, this one's in `tasks/tui-revamp/todo.txt`.
- No silent deviations: a different key or a missing feature is a `differs` or `na` row with its
  one-line `deviation`.
- `tests/parity.rs` fails when `keymap::BINDINGS` and the manifest's
  `tui.status = "done"`/`"differs"` rows disagree (keys and scope), or a `differs`/`na` row has no
  deviation. The Help screen renders from the manifest.

## Invariants
- Thin client: talks to the daemon, never parses the file — every byte painted comes from
  `GetFile`/`Watch`; the only file-shaped work done in-process is `txtodo_core::tokenize` for
  colouring (design §7 explicitly allows this: "identical token boundaries everywhere").
- Every buffer change is an `Apply`; this crate never writes `todo.txt` itself.
- **`SyncStatus` shipped 2026-09-18** across three slice-fenced sessions (proto message +
  RPC, `crates/txtodo-daemon/src/devices_grpc.rs::sync_status_impl`, then this crate's own
  `Daemon::sync_status` + `app.rs`'s 1 s tick). `ui/sync.rs` needed zero changes — it was already
  built and tested against `state.rs`'s `SyncSnapshot` fixture type, deliberately kept separate
  from the generated `pb` type so this crate never needed the proto change just to build.
  **Known gap the daemon handler surfaced, not fixed by it:** a device's `last_seen_ms` is set
  once at pairing registration and never advanced by any real sync session anywhere in
  `txtodo-daemon` — so every peer's `lag_ms` here currently reads as "time since it was paired",
  not "time since it was last actually reached". See `devices_grpc.rs::sync_status_impl`'s own
  doc comment and `tasks/tui/notes.md` for the full account; a follow-up task is spawned to wire a
  real touch on session success.
  Still blocked by the same "seeding needs an out-of-`allowedDeps` crate" reasoning: a real
  `needs_review` conflict flag for an integration test (`crates/txtodo-daemon/tests/grpc.rs::
  raise_flag` needs `txtodo_store` directly) and a real two-loopback-daemon *pairing* test for the
  `s` indicator (the RPC itself is now proven end to end against a real daemon with no peers,
  `tests/sync_status.rs` — what's still open is driving a real pairing handshake between two real
  daemons in this crate's own test harness) — both `ui::conflicts`/`ui::sync`'s own logic stay
  unit tested against fixtures for the peer-bearing case.
- Logs carry ids, counts and hashes — never line text, tokens or payloads.
- May depend only on: txtodo-core, txtodo-proto, txtodo-telemetry, txtodo-daemon-launch,
  txtodo-workspace-paths (external: ratatui, crossterm, tonic, tokio, hyper-util, tower, jiff —
  same socket-dial set `txtodo-cli` already carries, `cargo deny check` clean as of 2026-09-13,
  human sign-off still outstanding).
