# QR pairing and widgets on both mobile platforms (plan M9, plan §7)

Plan M9: "Both: QR pairing, widgets." Pairing reuses M4's `txtodo pair` flow
([../sync-pairing](../sync-pairing/notes.md)): QR carries `device`, `group_id`, X25519 public key,
endpoint, nonce — no secrets — then both humans compare the 6-word SAS and confirm. Widgets are
thin views over the local store, refreshed by the daemon, not a second live connection.

## QR pairing

- Android scans with CameraX / ML Kit barcode; iOS with AVFoundation. The decoded payload is the
  same struct as `txtodo pair <code>` base32 — one code path, two cameras.
- SAS confirmation is mutual (both humans compare, both press yes) before the group key is
  derived; a photographed QR is useless without the SAS. Reuse the M4 transcript exactly — the
  SAS must commit to both identities or it does nothing.

## Widgets

- iOS (design §7): WidgetKit widget + Live Activity for tasks due today, watchOS complication
  showing open count. https://developer.apple.com/documentation/widgetkit
- Android (design §7): AppWidget list of due-today tasks, Quick Settings tile for quick-add.
  https://developer.android.com/develop/ui/views/appwidgets
- Widgets read a local snapshot the daemon writes on every reconcile; tapping opens the app at the
  tapped line. No widget talks to the network itself.

## Acceptance

Pairing on both platforms reaches the same SAS words from the same transcript; a MITM transcript
change yields different words (sync-pairing's test); the decoded QR struct holds no key material;
both widgets render due-today tasks from the snapshot and open the right line.
