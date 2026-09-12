# Encrypt `Ops` with the group key (XChaCha20-Poly1305), sign with the device key (Ed25519) — M4

Sits on top of [sync-protocol-frames](../sync-protocol-frames/notes.md). Keys come from
[sync-keystore](../sync-keystore/notes.md); nothing here ever reads a key from a file path or an env
var directly.

## Signing and encryption answer different questions — keep them apart

The schema already decided half of this: `ops.signature BLOB`
(`crates/txtodo-store/migrations/0001.sql:12`) is a **per-op, durable** column, not a transport
field. That is the right shape, so:

- **Signature = authorship, per op, forever.** Ed25519 over the op's canonical bytes, stored in the
  row, verified on import *before* insert and re-verifiable years later during `blame`.
- **Encryption = confidentiality, per batch, in flight.** XChaCha20-Poly1305 over the serialised
  `Ops` message. It is stripped at import; the log on disk is plaintext (the SQLite file is already
  as secret as the todo.txt beside it).

This sidesteps the sign-then-encrypt / encrypt-then-sign argument entirely: the signature is not a
transport property, so surreptitious forwarding buys an attacker nothing — a re-encrypted batch
still carries each op's original author signature, and a group member cannot mint ops as someone
else.

## Canonical bytes — the thing that breaks later

An Ed25519 signature is over exact bytes, so the signed encoding must be deterministic. postcard is,
*for a given struct shape*. Three ways that quietly stops being true:

- A `HashMap`/`HashSet` anywhere in the signed payload — iteration order is not stable. Use
  `BTreeMap` or a sorted `Vec` in anything signed.
- Adding or reordering a field in `Op` or `OpKind`. Every stored signature becomes unverifiable.
  `OpKind` is already documented as a closed, append-only set; this task makes that a *security*
  invariant, not a style one. Say so in the doc comment.
- Signing the `Op` including its own `signature`. Define `Op::signing_bytes()` explicitly and give
  it a golden test, rather than relying on "we remember to clear the field".

Ref: <https://docs.rs/ed25519-dalek> · RFC 8032 <https://www.rfc-editor.org/rfc/rfc8032>.

## The AEAD

`chacha20poly1305::XChaCha20Poly1305` (<https://docs.rs/chacha20poly1305>, XChaCha draft:
<https://datatracker.ietf.org/doc/html/draft-irtf-cfrg-xchacha-03>). The 24-byte nonce is the whole
reason to pick XChaCha over ChaCha: random nonces are safe at our volumes, so there is no nonce
counter to persist and no way to reuse one after a restore-from-backup. Generate it fresh per batch
from the OS CSPRNG, never from the injected test PRNG — the seeded PRNG used by the simulator must
not be reachable from this path, and a test asserting that is worth writing.

Bind context with the AAD, do not leave it empty: `version || group_id || key_epoch`. That makes a
frame from another group, or a downgrade to an older protocol version, fail the tag check rather
than decrypt into something plausible.

## Key epochs, because device removal rotates

`txtodo device remove` rotates the group key and old ops stay readable under the old key. So the
ciphertext header carries `key_epoch: u32` in the clear (it is also in the AAD). Decrypt looks the
epoch up; an unknown epoch is a typed error naming it, never a retry loop over every key we hold.
Keep at most `MAX_RETAINED_KEY_EPOCHS`, asserted.

## Failure handling

Every failure here is an attack or a bug, never a normal condition — but none of it may panic:
this is external input, so it is **validated**, not asserted (CLAUDE.md §3). One typed error enum,
each variant naming what failed and against which epoch/device. A batch with one bad signature is
rejected whole; partial import of a signed batch is not a thing.

## Tests

- Tampering with one byte of a signed op fails verification (the acceptance criterion in plan M4).
- A peer without the group key cannot decrypt: wrong key, wrong group in AAD, wrong epoch — three
  separate cases, three distinct errors.
- `signing_bytes` golden: checked-in bytes for a fixed `Op`, so a field reorder fails loudly.
- A `HashMap` in a signed payload is caught — assert `signing_bytes` is stable across 100 runs in
  one process (hash seeds differ per process, so also across two).

## As built (2026-09-12, agent)

- `crates/txtodo-model`: `Op::signing_bytes()` (postcard of the op; there is no `signature` field to
  strip — it lives in `ops.signature`, never inside `Op`), golden `goldens/op_signing.postcard`, and
  the `OpKind` append-only security note. Audited: no `HashMap`/`HashSet` anywhere reachable from
  `Op`.
- `crates/txtodo-sync/src/sign.rs`: `DeviceSigningKey` (redacted `Debug`, injected, never read from
  disk), `DevicePublicKey`, `Signature`, `sign`/`verify`, and all-or-nothing `verify_batch` over
  parallel `ops`/`signatures` slices. The origin is `op.hlc.device`, so a signature cannot be
  re-attributed to another device by editing the op.
- `crates/txtodo-sync/src/aead.rs`: `GroupKey`/`GroupKeys`, `seal`/`open`. Nonce comes straight from
  `getrandom`; there is no RNG parameter, so the simulator's seeded PRNG has no path in. AAD is
  `version || group || epoch`; the clear header is `version || group || epoch || nonce`.
- `crates/txtodo-sync/src/crypto_error.rs`: one `CryptoError`, every variant names the epoch, device
  or group it failed against. All failures are validated (`Err`), nothing panics.
- `cargo deny check` run 2026-09-12: `advisories ok, bans ok, licenses ok, sources ok`. ed25519-dalek
  2.2.0 (BSD-3-Clause) and chacha20poly1305 0.10.1 (Apache-2.0/MIT) are both allowed by `deny.toml`.
- Judgement calls, flagged for the human:
  - The clear header carries `version` and `group` as well as `epoch`, so "wrong group in the AAD"
    is a distinct `WrongGroup` error rather than an indistinguishable tag failure. The notes only
    promised `epoch` in the clear; the extra two fields are public (they are in `Hello`) and make the
    required three distinct errors possible.
  - `MAX_RETAINED_KEY_EPOCHS = 16` (no number was specified). The keystore's `MAX_STORED_EPOCHS`
    should re-use this constant, per its notes.
- Not in this slice (named so the next one can pick them up): carrying signatures on the wire
  (`Message::Ops` still holds only `Vec<Op>`), persisting them via the `ops.signature` column
  (`Store::append` omits it), and the daemon import path that calls `verify_batch` before insert.
  The crypto contract — verify the whole batch, insert nothing on one bad signature — is in
  `verify_batch`'s doc and is enforced there; only the call site is pending.
