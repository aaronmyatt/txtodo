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
- Devices: pairing from this side works by pasting the other device's code, then comparing six words ("They match, my device" merges default workspaces; "not mine" keeps them apart; "They differ" stops). Showing this device's code and QR came later (below). Paired devices list and revoke; workspace offers are listed here too (the `o` pane still works).
- Devices, this device's code (2026-09-27, after the pairing-code decide line):
  - "Show a code" sends `PairOffer`. The code shown is `PairOfferResponse.code`, made by the daemon;
    Enter on it copies it (OSC 52). The QR holds the offer's JSON payload, built here by hand in
    `pair_code.rs` (nine string fields, serde_json's escapes, the CLI's field order), drawn with
    `qrcode`'s half-block renderer under the rows. A pane too small for it says how big it must be.
  - The countdown is 120 s, not the 60 s the line said: that is the daemon's window
    (`PAIRING_WINDOW_MS`), and desktop and `txtodo pair` use it too.
  - The 1 s tick polls `PairAwaitPeer`; once a device joins, its six words show with the usual They
    match / They differ rows. Past the window the code is dropped with a toast.
  - A daemon older than the code field sends it empty: the status line then says to run
    `txtodo pair`, rather than the TUI encoding the code itself.
  - Known gaps: the QR is drawn in the terminal's own colours, as `txtodo pair` does, so a scanner may
    need to read it inverted on a light theme. Not driven against a second real device here: the
    unit tests cover the payload, the rows, the copy and the drawing only.
- Tokens: the "dialog" is one typed line, `name scope… expires:YYYY-MM-DD` (read when no scope); the secret shows once, Enter copies it (OSC 52) and hides it.
- Tests: `settings_rows_tests.rs`, `commands_settings_tests.rs`, `prefs.rs`, `manifest.rs`, and `tests/settings_screen.rs` against a real daemon.
