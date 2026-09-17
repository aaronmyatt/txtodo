# Pairing code: compact encoding instead of raw JSON paste

## Goal

Replace the text-fallback half of `txtodo pair` — today a raw `serde_json`-serialized
`PairOfferResponse`, printed for copy-paste when QR scanning isn't used — with a compact,
non-JSON-looking code, without touching the crypto or protocol underneath it at all.

## Why this is worth its own ticket, not just polish

Two desktops don't have a camera pointed at each other's screen the way two phones do. That makes
the JSON-paste path the **primary** desktop-to-desktop pairing mechanism today, not a rare
fallback — see `crates/txtodo-cli/src/commands/pair.rs`'s `PairingCode::to_json`/`from_json` and
the workflow codified in `docs/trial-run-two-computers.md`:

```
txtodo pair            # on A: prints a JSON offer, waits
txtodo pair '<JSON>'   # on B: paste it, prints six SAS words
```

That's the literal "sharing the JSON blob is cumbersome" pain point. Fixing it is a real
desktop-general-availability gap, not cosmetic.

## Design

- Same fields, same crypto: `PairOfferResponse` (`device`, `group_id`, `x25519_pub`, `endpoint`,
  `nonce`, `identity_mode`, `relay_node_id`, `relay_url` — no key material, per the existing
  proto) is untouched. Only its *text* encoding changes: `postcard::to_allocvec` (the same crate
  `txtodo bundle`'s wire format already uses, `crates/txtodo-daemon/src/bundle_wire.rs`) then
  base32 (Crockford, matching `WorkspaceId`/ULID's own alphabet already used throughout the repo)
  instead of `serde_json::to_string`.
- The SAS (6-word) confirmation step that follows is completely unchanged — this ticket only
  shrinks and de-JSONs the one string a human has to move out-of-band. Security property (both
  humans confirm the same 6 words before the group key is trusted) is untouched.
- No new server, no `relay/` blob-mailbox deployment. Considered and rejected for this pass: having
  the initiator `PUT` the offer to `relay/`'s (currently undeployed) ciphertext blob store keyed by
  a short code, with the joiner `GET`-ing it by code. That removes copy-paste entirely, but needs
  `relay/` actually deployed somewhere both devices can reach — real new infrastructure, which
  contradicts "shortest path, no self-hosting" for this dogfooding pass. Worth revisiting later if
  even the compact code is still too much friction.

## Explicitly not this ticket

- No change to `PairOfferResponse`'s proto shape or the pairing gRPC calls.
- No change to QR generation — QR already works fine; this only touches the text fallback.
- No back-compat with the old JSON format — matches the project's stated "no released users yet"
  posture used elsewhere (`daemon-device-set-identity`'s notes.md) for breaking a wire-adjacent
  format with no migration story.

## As built

`run_offer`'s printed fallback is now `to_compact(&code)` (postcard+base32); the QR itself still
renders JSON unchanged.

Deviated from the original "no back-compat" plan after finding a real consumer mid-implementation:
`apps/desktop`'s own TypeScript QR encode/decode (`qr.ts`/`pairing.ts`) independently builds and
scans JSON — decode now accepts either format (auto-detected by a leading `'{'`), so the desktop
app needed zero changes; only the CLI's own encode side switched.

Verified for real: the existing two-daemon `pairing.rs` suite (4/4) now exercises the compact path
in normal use, no separate test needed.
