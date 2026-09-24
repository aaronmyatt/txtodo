# tui-settings

## Goal
- The seven c2 Settings cards as one TUI screen.

## Design
- Follow `apps/desktop/design-mockups/c2/settings.js`: a left card nav and scrolling cards. `/` filters cards by keyword.
- Rows that can't exist here are shown as not available, with the manifest's deviation:
  - pin, menu bar, font, text size, line height, camera scan, folder picker.
- Pairing:
  - The QR is drawn with Unicode half-blocks using `qrcode`. The dep needs the decide line.
  - A code can also be pasted.
  - The six SAS words are confirmed with They match / They differ.
- Paired devices use `DeviceList` / `DeviceRemove`, which already exist in the proto; desktop's revamp says "no RPC", which is wrong.
- The token secret is copied with OSC 52 (a terminal escape sequence that sets the clipboard).
- TUI prefs are stored in a TUI-owned file.
  - Desktop uses localStorage, so preferences are per client. This is recorded as `differs`.

## As built
- Screen (`ui/settings.rs`): card nav on the left (the cards `/` keeps), the card as rows of label and value. Keys: j/k rows, Tab / l / Right and Shift-Tab / h / Left cards, Enter or Space run the row, `d` twice removes, `/` filters. A screen's own key now beats a global one (`keymap::Chords::resolve`), so `/` here filters instead of searching.
- Rows (`settings_rows.rs`): one list per card, from state; `Act` for Enter, `remove` for `d`, `field` rows take typing first. Rows a terminal cannot have say so, dim.
- Daemon side (`app_settings.rs`): refresh on entering Settings; the activity feed is read only on its card (the stream replays the log, then waits: read until a 300 ms pause, newest 200 kept).
- Preferences (`prefs.rs`): `$XDG_CONFIG_HOME/txtodo/tui.conf` (else `~/.config/...`), `key = value`. Theme applies at once; line numbers and the underline change the Tasks rows. Desktop keeps its own in localStorage.
- Shortcuts and Help read `specs/client-parity.toml` built into the binary (`manifest.rs`); `tests/parity.rs` uses the same parser now.
- General: Restart is an instruction (`txtodo daemon stop`, then `start`); the TUI does not stop the daemon itself.
- Devices: pairing from this side works by pasting the other device's code, then comparing six words ("They match, my device" merges default workspaces; "not mine" keeps them apart; "They differ" stops). Showing a code or QR here needs the code encoding, which lives in the CLI (postcard + base32 + qrcode): blocked on a decide line. Paired devices list and revoke; workspace offers are listed here too (the `o` pane still works).
- Tokens: the "dialog" is one typed line, `name scope… expires:YYYY-MM-DD` (read when no scope); the secret shows once, Enter copies it (OSC 52) and hides it.
- Tests: `settings_rows_tests.rs`, `commands_settings_tests.rs`, `prefs.rs`, `manifest.rs`, and `tests/settings_screen.rs` against a real daemon.
