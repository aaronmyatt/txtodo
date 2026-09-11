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
