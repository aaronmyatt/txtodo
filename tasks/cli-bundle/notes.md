# txtodo bundle export|import (plan M8)

Design §4.5 ("Sneakernet" row): `txtodo bundle export` / `txtodo bundle import`, git-bundle style —
one self-contained file carried by hand (USB stick) to bootstrap an air-gapped device.

## What goes in the bundle

Snapshot + op tail (the `snapshots` + `ops` rows the op log already holds), a manifest (device id,
schema version, protocol version, per-file hashes), and the group key so a fresh device can join
and decrypt. The group key is the one secret: wrap the bundle with a passphrase-derived key
(Argon2id → XChaCha20-Poly1305). The physical medium is the sneakernet security boundary; the
passphrase is the second line. Flag to the human — the design says only "git-bundle style" and does
not state whether the bundle carries key material.

## Export

Stream, do not buffer: dump snapshot + ops incrementally (design §4.4 keeps full history by
default). Write the manifest first so import can validate before reading the rest.

## Import

Verify before insert, exactly like a normal batch: per-op Ed25519 signature
([sync-crypto-envelope](../sync-crypto-envelope/notes.md)), version match, hashes match. Then insert
ops and materialise. Tampered or truncated ⇒ a typed error naming what failed, never a partial
import. Manifest carries a `version` field from day one (same rule as
[sync-protocol-frames](../sync-protocol-frames/notes.md)).

## Tests

- Export on A, import on a fresh B ⇒ identical file bytes and op log state.
- One flipped byte anywhere fails import with a distinct error, no partial state.
- A nested-ref workspace round-trips reproducing the whole tree.
