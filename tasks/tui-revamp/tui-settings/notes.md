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
