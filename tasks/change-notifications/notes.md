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
