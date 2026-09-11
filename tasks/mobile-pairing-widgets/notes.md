# QR pairing and widgets on both mobile platforms (plan M9, plan §7)

## Goal

Plan M9: "Both: QR pairing, widgets." Pairing reuses M4's `txtodo pair` flow exactly
([sync-pairing](../sync-pairing/notes.md)) — a QR carrying no secrets, plus a mutually-confirmed
6-word SAS — on Android (CameraX) and iOS (AVFoundation). Widgets are thin views over a local
snapshot the daemon writes on every reconcile; none of them talks to the network.

## Design

### QR pairing — one payload, two cameras

The decoded payload is the same struct `txtodo pair <code>` takes base32-encoded; Android and iOS
scan it into the identical shape. The payload carries no key material (a photographed QR must be
useless without the SAS confirmation).

```kotlin
// Shared pairing payload (Kotlin) — Swift is the mirror image, field-for-field.
// None of these fields is confidential (sync-pairing §"The QR carries no secrets").
data class PairingPayload(
    val device: String,        // device id, not a key
    val groupId: String,
    val x25519Pub: ByteArray,  // the initiator's PUBLIC key
    val endpoint: String,      // LAN endpoint or relay address
    val nonce: String,         // one-time pairing nonce, single use
)
```

- The SAS is `HKDF-SHA256(ikm = x25519(priv, peer_pub), salt = transcript,
  info = b"txtodo-sas-v1")` where `transcript = protocol_version ‖ device_a ‖ pub_a ‖ device_b ‖
  pub_b ‖ group_id`; index `sas_bytes` into the vendored EFF short list (1296 words, 6 words ≈ 62
  bits). It commits to *both* identities — the whole point (sync-pairing). Both humans compare,
  both confirm; the group key is sent only after both confirmations, under
  `info = b"txtodo-pair-v1"` (never reuse one derived key for two purposes).
- Android scans with CameraX (https://developer.android.com/training/camerax) or ML Kit barcode;
  iOS with `AVFoundation` `AVCaptureMetadataOutput`.
- Bounds mirror sync-pairing: `PAIRING_WINDOW_MS` (default 120 000), single-use nonce,
  `MAX_CONCURRENT_PAIRINGS` asserted, rate-limited SAS confirmations.

### Widgets — snapshot readers, never live clients

The daemon writes a local snapshot on every reconcile; widgets read it and open the app at a line.
No widget opens a socket or parses a file.

- iOS (design §7): WidgetKit widget + Live Activity for tasks due today; watchOS complication shows
  the open count. https://developer.apple.com/documentation/widgetkit
- Android (design §7): `AppWidgetProvider` list of due-today tasks; Quick Settings tile (`TileService`)
  as a quick-add entry point. https://developer.android.com/develop/ui/views/appwidgets
- Tapping a widget item deep-links to the app at the tapped line; the Quick Settings tile opens the
  quick-add/edit popover.

## Placement/dependencies

- No new crates; work lives in `apps/android/` and `apps/ios/` (both non-frozen). Depends on
  `sync-pairing` (transcript/SAS/wordlist invariants) and the shared snapshot written by the
  daemon reconcile path. Android Quick Settings tile needs `android.permission.BIND_QUICK_SETTINGS_TILE_SERVICE`.

## Edge cases & invariants

- MITM: an attacker running two handshakes must not make both sides show the same words — the SAS
  binds the whole transcript (sync-pairing's acceptance test is the guarantee; re-run it with the
  mobile payload builders).
- A photographed/replayed QR is useless without the SAS; assert on the *decoded struct's fields*
  that no key material is present, so a future field addition is reviewed.
- Widget snapshot reads must be non-blocking and bounded (render the first N due-today tasks, cap
  the snapshot size); a missing snapshot renders an empty state, never a spinner forever.
- The vendored EFF wordlist is byte-identical to the one `sync-pairing` vendors (count 1296, no
  duplicates, same file hash) or the two implementations disagree silently.

## Acceptance

- Same transcript → same 6 SAS words on Android and iOS; any single-bit change to a transcript
  field changes the words.
- The decoded QR payload contains no key material (assert on decoded fields).
- Pairing reaches the group-key state only after mutual SAS confirmation on both platforms.
- Android widget + iOS widget/Live Activity/watchOS complication render due-today tasks from the
  snapshot and open the right line on tap.

## References

- plan M9 (txtodo-implementation-plan.md); design §7 Android/iOS rows + §4.6 pairing (txtodo-design.md)
- [../sync-pairing/notes.md](../sync-pairing/notes.md) (transcript, SAS, wordlist, MITM test)
- https://developer.apple.com/documentation/avfoundation/avcapturemetadataoutput
