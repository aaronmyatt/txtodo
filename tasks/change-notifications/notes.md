# Notify on changes made elsewhere, debounced

## Goal
- Each long-running client (desktop, TUI) tells the human when a task or sub-task is edited,
  deleted or completed by someone else: another device, or another client on this device (an
  agent over MCP, the CLI, an editor save).
- Debounced: a burst (an agent closing ten lines) is one notice, not ten.
- Asked by the human 2026-10-11.

## Design
- No proto change. The daemon's `Watch` stream already sends `Change { path, ops[] }`, and each
  `OpSummary` carries `device`, `source`, `kind` and a one-line `summary`.
- "Someone else" = an op whose `device` is not ours, or whose `source` is not this client's own
  ("desktop" / "tui"). Our own edits never notify.
- Kinds: `edit_text`/`set_field` = edited (a `set_field` on done = completed), `blank_insert` /
  `remove` = deleted. Insert and move do not notify (noise).
- Debounce: collect for 3 s after the first op, then one notice: one op → its summary; many → "N
  changes in todo.txt, tasks/foo/todo.txt" style count.
- Desktop: an OS notification via tauri-plugin-notification, only while the window is not focused;
  a Settings toggle, default on.
  Ref: https://v2.tauri.app/plugin/notification/
- TUI: a toast (it has no OS notifier); no toggle needed, toasts already fade.
- CLI: no long-running process, nothing to do.

## Open questions
- None blocking. The kind → verb map is checked against the store's real tags when built.

## As built
- Found 2026-10-11: Watch ops had an empty `source` (only History and the activity stream filled
  it). The daemon now carries the commit's source on `handle::Change` into each Watch op
  (`op_source.rs::watch_carries_the_source_too`).
- So "someone else" needs no device id: another device's ops arrive as `sync`, an agent's as `mcp`,
  an editor save as `external`. A client drops only ops whose source is its own name. Known gap:
  two TUIs (both `tui`) never notify each other.
- `txtodo_core::notice`: `verb_of` reads the verb off `kind` + `summary`; `Debouncer` keeps the
  strongest verb per task, sends after 3 s quiet or 15 s max, and words "A task was completed by
  an agent" / "3 tasks changed on another device" + counts and lists. No line text: Watch ops
  don't carry it. A daemon test (`convert_tests.rs`) pins the summary wording it reads.
- TUI (`app_notices.rs`): every Watch change feeds `Shell::notices`; its due time joins the
  loop's deadlines and the notice shows as a toast ("A task was completed by an agent · in
  todo.txt"). The TUI watched only its open lists before; it now watches every list (empty
  `paths`, as the desktop does), or a sub-list edit elsewhere would never reach it.
- Desktop: waits on a human sign-off for `tauri-plugin-notification` (new dependency, the
  Cargo.toml rule). Plan once signed off: register the plugin; in `commands.rs`' Watch forwarder
  push each change into a `Debouncer::new("desktop")` held in `AppState`; one tokio task sleeps to
  `next_due`, then shows the notice unless the main window is focused or the Settings toggle
  (localStorage, mirrored to Rust with a `set_change_notices` command, like `pin.ts`) is off.
