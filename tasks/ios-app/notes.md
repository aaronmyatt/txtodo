# iOS SwiftUI over UITextView, embedded daemon, BGAppRefresh, silent push, App Intents, share sheet (plan M9, plan §7)

Design §7 iOS row: SwiftUI + uniffi core, widgets, Live Activities, App Intents / Shortcuts,
watchOS complication, share sheet. The daemon is embedded in the app process via the
`DaemonHandle` from [../ffi-uniffi-daemonhandle](../ffi-uniffi-daemonhandle/notes.md).

## Why UITextView

SwiftUI's `TextField` edits plain strings and cannot preserve attributed spans while typing. The
edit popover (and notes editor) is a `UIViewRepresentable` over `UITextView` with an
`NSAttributedString` built from `tokenize` spans, so highlighting and editing live in one view.
https://developer.apple.com/documentation/uikit/uitextview

## Rendering (§3.1–3.2)

- Line list renders `todo.txt` with line numbers; `id:` filtered from the attributed string, shown
  on toggle; completed lines muted + strikethrough.
- Single tap → edit popover with chips; double tap → detail view (breadcrumb, pinned parent,
  `notes.md` editor, recursive sub-list, `n of m done`).

## Background + system integration

- `BGAppRefresh`: schedule a background task that drives a daemon reconcile, then update widgets.
  https://developer.apple.com/documentation/backgroundtasks/bgapprefreshtask
- Silent push: APNs silent notification wakes the app to reconcile (the M8 relay forwards APNs).
- App Intents `AddTodo` / `CompleteTodo` / `ListTodos` call the `DaemonHandle`.
  https://developer.apple.com/documentation/appintents
- Share sheet: accept text and add it as a todo through the daemon.

## Acceptance

`tokenize` → `NSAttributedString` span mapping is unit-tested and byte-identical to the Android
mapping; App Intents run in Shortcuts; BGAppRefresh handler reconciles and exits.
