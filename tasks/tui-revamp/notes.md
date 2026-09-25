# tui-revamp

## Goal
- The TUI does what the desktop app does, 1-to-1, to the c2 spec in `tasks/desktop-ui-revamp/notes.md`.
  - Switching clients should feel incidental.
  - Every TUI key works in desktop. Every desktop capability exists in the TUI.
- The TUI may present things differently: screens and popups instead of sliding side panels.
  - Pin, tray, the OS-global hotkey and camera scan are `na` in the TUI.
- Rich mouse support.
- TUI goes first. It is the reference until the desktop revamp resumes.

## Decisions (2026-09-24, human)
- **List mode on both clients.**
  - Esc leaves text edit. Then j/k/x/dd/J/K/gg/G/Enter act on lines. `i`/`a`/typing edits.
  - Desktop gains this mode as `@parity` lines.
- **TUI first.** Desktop rows in the manifest stay `planned`.
- **Shared logic lives in `txtodo-core`.** Desktop reaches it through the `txtodo-ffi` wasm build.
  - Search matcher, strict hints, chip edits, Universal grouping.
  - Plus a new `UniversalTasks` daemon RPC.

## Decisions (2026-09-25, human)
- **ADR 0031 signed.** The manifest convention is accepted as written; the ADR's status is now `accepted`.

## Rejected
- **Opt-in vim mode on desktop**: the two clients behave differently until the toggle is on.
- **A copy of the logic per client, checked by fixture tests**: the same logic lives in two places and drifts.
- **Lockstep builds**: too slow while the desktop revamp is deferred.

## Design
- **Anti-drift.**
  - `specs/client-parity.toml` is the one list of user-facing actions. See `parity-manifest/`.
  - Tests in both clients fail when it and the code disagree.
  - Help screens render from it.
  - An `@parity` line goes to whichever client lags.
- **Order.**
  - `parity-manifest`
  - `shared-core`, then `universal-rpc` (other crates, one fenced session each)
  - `tui-foundation`, then `tui-mouse`
  - then the screens
- Thin client holds. All data comes from daemon RPCs; core is used only for pure logic.

## Screen map (desktop → TUI)
- Header workspace dropdown → `W` popup (click the workspace name too).
- Sidebar Activity tab / Settings Activity card → Settings › Activity.
- Detail bottom panel → bottom split panel (same).
- Conflict banner + review sheet → banner + modal sheet (replaces the `r` pane).
- Prompt bar → prompt bar.
  - The Quick Add always-on-top window is `na`.
- `/universal` → Universal screen (`g u`).
- `/settings#card` → Settings screen with a card nav (`g s`).
- `/help` → Help screen (`?`).
- Status footer → status footer.
  - The `s` sync pane becomes a popup from it.
- The TUI `o` offers pane → Settings › Devices.

## Desktop @parity lines to file (tui-revamp line "File the @parity catch-up lines")
- List mode in the Tasks buffer: Esc leaves text edit, j/k/x/dd/J/K/gg/G/Enter act on lines, i/a/typing edits.
- u undo in list mode via the daemon Undo.
- Live sync indicator (SyncStatus) in the status footer.
- Workspace offers (pending/accept/decline) in Settings › Devices.
- Drag a line to reorder.
- Universal via the UniversalTasks RPC; drop commands_universal.rs's local parse.
- Paired devices list + Revoke via DeviceList/DeviceRemove (the proto already has both).
- Search, strict hints and chip edits through the wasm core exports; drop the TS copies.
- Help page and Shortcuts card render from specs/client-parity.toml.

## Open questions
- The `Decide:` lines at the top of `todo.txt`: reopen, new deps, gate script, detail panel, pairing code. (ADR 0031 was signed 2026-09-25.)
- The TUI prompt-bar key.
  - Plan: `Ctrl-Space`, plus `Ctrl-Shift-Space` where the kitty keyboard protocol works.
  - Desktop uses `Mod-Shift-Space`. The manifest should record this as `differs` unless a better shared key turns up.

## As built
- 2026-09-25: every sub-backlog that needs no human is built: foundation, mouse, shell, Tasks, detail panel, prompt bar, Universal, Settings, Help; the desktop @parity lines are filed. Each folder's notes.md has its own "As built".
- Open, all waiting on a person: the five Decide lines at the top of todo.txt (reopen, deps, gate script, detail panel, and the new pairing-code one; ADR 0031 was signed 2026-09-25), the human pass, check-parity.sh, and showing this device's pairing code.
- Found on the way and fixed: other clients' edits never repainted the TUI until a reconnect; Space on a done line never reopened it (Complete leaves done lines alone; the TUI now sends Reopen).
- Reopen already exists in the proto and daemon (2026-09-20), which the reopen decide line predates.
