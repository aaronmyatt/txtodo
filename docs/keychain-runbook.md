# Runbook: offers from paired devices are blocked (macOS keychain)

Task `control-channel-keystore-visibility`. The signing decision stays where
`tasks/relay-id-keystore/notes.md` put it (self-sign with `scripts/macos-selfsign.sh`); this page is
only how to get unstuck.

## What you see

- `txtodo doctor` has a Warn row: `offers  blocked (Ns ago): the keystore could not read the group
  key: … the OS keychain did not answer the read within 20s (a permission prompt nobody answered?)`.
- `txtodo workspace offers` prints the same line to stderr. The TUI `o` pane says `offers blocked`.
  The desktop Devices page says workspaces from your other devices are not arriving.
- A workspace you use on another device never shows up here as a Remote workspace.

## Why

- The daemon keeps the sync group key in the login keychain: service `txtodo`, account
  `device/group-epoch-0` (next to `device/device-static`, `device/device-signing`,
  `device/relay-identity`).
- macOS ties "Always Allow" to the binary's code signature. An ad-hoc signed build gets a new
  signature on every rebuild, so the keychain asks again.
- Under launchd (`com.txtodo.txtodod`) nobody sees that prompt. Each read waits 20 s, fails, and
  the offer exchange stops. It retries every 15 s and fails the same way until someone answers.

## Fix: answer the prompt once

1. Stop the launchd copy so it stops retrying:

   ```bash
   launchctl bootout gui/$(id -u)/com.txtodo.txtodod
   ```

2. Run the daemon in a terminal, so the prompt appears in front of you:

   ```bash
   ~/.cargo/bin/txtodod
   ```

3. When macOS asks whether `txtodod` may use the `txtodo` keychain item, enter your login password
   and choose **Always Allow**. It can ask once per item (up to four). Then stop it with Ctrl-C.
4. Start the launchd copy again:

   ```bash
   launchctl bootstrap gui/$(id -u) ~/Library/LaunchAgents/com.txtodo.txtodod.plist
   ```

5. Check: `txtodo doctor` shows `offers  no problem reported` within a minute (the next offer
   exchange clears it).

## Fix without a terminal prompt: Keychain Access

1. Open Keychain Access, login keychain, search `txtodo`.
2. For each `txtodo` item: Get Info, Access Control, add `~/.cargo/bin/txtodod` (Cmd-Shift-G to type
   the path), Save Changes.
3. Restart the daemon: `launchctl kickstart -k gui/$(id -u)/com.txtodo.txtodod`.

## Do not delete the item

- `security delete-generic-password -s txtodo -a device/group-epoch-0` removes the group key. The
  device then no longer belongs to its sync group: you would have to pair it again.
- Deleting `device/relay-identity` changes this device's relay node id, which breaks any relay
  allowlist that names it.

## After every rebuild

- A self-signed build (`scripts/macos-selfsign.sh`) keeps the same signature, so "Always Allow"
  should survive a reinstall. Not re-checked after a self-signed reinstall yet (see
  `tasks/relay-id-keystore/notes.md`, 2026-09-24).
- An ad-hoc build asks again: repeat the fix above after installing it.
