# txtodo-tui

## Purpose
The ratatui client, at desktop parity with the c2 spec (task `tui-revamp`, 2026-09-25): header,
banners, the Tasks screen with its detail panel, Universal, Settings, Help, the prompt bar, the
status footer, toasts, full mouse. Every change goes through the daemon; the plan and each
screen's as-built notes are in `tasks/tui-revamp/*/notes.md`.

## Public interface
- Input: `input::Input` (keys, mouse) → `keymap` (one `commands!` table in `keymap/table.rs`:
  `Command`, manifest ids, keys, scope; `Chords` for `g g`-style chords) → `commands::run` (and
  `commands_nav`, `commands_detail`, `commands_universal`, `commands_settings`, `search`, `prompt`)
  → an optional `action::Action` → `app::perform`, the one place an `Action` becomes an RPC.
  `mouse::Mouse` looks events up in the last frame's `hit::HitMap`.
- State: `state::AppState` (the open list, cursor, scroll, editor, flags) plus `state_nav`
  (screen, focus, overlay), `state_shell` (workspace, search text, `W` menu, link, banners,
  toasts, the change stack `u` and Undo use), `state_tasks` (ref badges), `state_detail` (the
  panel's levels; `with_sub_list` swaps a sub-list in so list mode runs on it unchanged),
  `state_universal`, `state_settings`. `AppState::fixture()` for tests.
- Drawing: `ui::screen::draw` composes header, banners, the screen, the prompt bar, the footer,
  toasts and popups, and returns the `HitMap`. One module per part under `ui/`; `paint` and
  `theme` colour tokens (desktop's palette as RGB, or a 16-colour table by hue).
- Daemon: `daemon::Daemon` plus one wrapper file per area (`daemon_workspace`, `_notes`,
  `_history`, `_devices`, `_tokens`, `_activity`). `app_*` modules hold each screen's async work
  (`app_apply`, `app_detail`, `app_universal`, `app_settings`, `app_workspace`, `app_refs`).
- `app_loop`: terminal input, the `Watch` stream (kept alive: a drop reconnects on the 1 s tick),
  the tick, and deadlines (notes autosave, the prompt hint). `terminal`: raw mode, mouse, kitty
  keys, all undone on exit and on panic.
- `manifest`: `specs/client-parity.toml` built in; Help and Settings › Shortcuts render from it.
  `prefs`: the TUI's own `tui.conf`. `clipboard`: OSC 52.
- Tests: unit tests per module (`AppState::fixture()`, hand-drawn hit maps, `TestBackend`);
  real-daemon tests under `tests/` are `#[ignore]`d: run them with
  `TXTODO_TEST_KEYSTORE_MEMORY=1 TXTODO_NO_AUTOSTART=1 cargo nextest run -p txtodo-tui
  --run-ignored all`.

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
- Thin client: every line painted comes from `GetFile`/`Watch`/`UniversalTasks`; in-process work
  on text is pure core logic only (`tokenize`, `query::matches`, `chips`, `strict_hint`,
  `universal::group`, `diff_text`), the same code desktop reaches through the wasm build.
- Every list change is an `Apply` (or `Undo`, `EditNotes`, `RefDir`); this crate never writes a
  todo.txt or notes.md itself. The one file it writes is its own `tui.conf` (`prefs.rs`).
- A line with a `needs_review` flag is read-only until the flag is resolved (per line; desktop
  locks its whole buffer).
- `u` and a toast's Undo take back only this session's changes, all of a change's ops
  (`ApplyResponse.applied`), in the workspace the change was made in.
- Known gaps, from the daemon side: a peer's `last_seen_ms` is set at pairing and not advanced by
  sync, so the sync popup's lag reads as time since pairing (`devices_grpc.rs::sync_status_impl`);
  `UniversalTasks` lists only workspaces the daemon has loaded; no test here drives two real
  daemons through pairing or raises a real `needs_review` flag (both need crates outside this
  one's allowed deps).
- Logs carry ids, counts and hashes — never line text, tokens or payloads.
- May depend only on: txtodo-core, txtodo-proto, txtodo-telemetry, txtodo-daemon-launch,
  txtodo-workspace-paths (external: ratatui, crossterm, tonic, tokio, hyper-util, tower, jiff —
  same socket-dial set `txtodo-cli` already carries, `cargo deny check` clean as of 2026-09-13,
  human sign-off still outstanding).
