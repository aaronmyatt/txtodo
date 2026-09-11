# Android foreground-service daemon, Compose UI with AnnotatedString from tokenize, LAN MCP, FCM (plan M9, plan §7)

Design §7 Android row: Compose + uniffi core, widgets, Quick Settings tile, foreground-service
daemon. The UI model is the file — Compose renders `todo.txt` as a highlighted document with line
numbers and `AnnotatedString` built from `txtodo_core::tokenize` spans, so boundaries match every
platform. `id:` hidden by default.

## Service owns the daemon

A foreground service (type `dataSync`) hosts the `DaemonHandle` from
[../ffi-uniffi-daemonhandle](../ffi-uniffi-daemonhandle/notes.md) so the daemon survives UI
death. The UI binds to it and drives it via the FFI. Foreground-service rules:
https://developer.android.com/develop/background-work/services/fgs

## Rendering (§3.1–3.2)

- Line list = `LazyColumn` of `AnnotatedString` rows. Each `SpanKind` → a `SpanStyle`; completed
  lines muted + strikethrough. `id:` filtered from the attributed string, shown on toggle.
  https://developer.android.com/reference/kotlin/androidx/compose/ui/text/AnnotatedString
- Single tap → edit popover: single-line field pre-filled with the raw line, token chips
  `(A)(B)(C) + @ due: t: rec: x` that insert/toggle tokens; strict-mode validation via the FFI
  parser, lenient save allowed (never block saving).
- Double tap → detail view: breadcrumb, pinned parent, `notes.md` editor, recursive sub-list,
  `n of m done` header. First keystroke into empty notes creates the directory (rule 4).

## LAN MCP + FCM

- MCP over HTTP on LAN while the service runs, reusing `txtodo-mcp`; refuse non-loopback unless
  `--lan` (security checklist).
- FCM: register the device token with the daemon so the M8 relay can forward wake-ups.
  https://firebase.google.com/docs/cloud-messaging

## Acceptance

Instrumented test asserts `tokenize` → `AnnotatedString` spans (id: absent, strikethrough on completed) on-device; MCP LAN call returns tool results; service survives activity recreation.
