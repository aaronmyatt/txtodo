# Pairing: `txtodo pair` shows a QR + 6-word SAS from an X25519 handshake, then snapshot + ops — M4

Plan M4: "`txtodo pair` shows a QR + SAS (6 words from the EFF short list) derived from an X25519
handshake; the other device runs `txtodo pair <code>` or scans. Result: both devices hold the group
key; new device receives a full snapshot then ops."

## The SAS must commit to both identities, or it does nothing

A short authentication string exists to catch an active machine-in-the-middle. It only does that if
an attacker running two sessions cannot make both sides show the same words. So the SAS is **not**
derived from the shared secret alone — it is derived from the whole transcript:

```
transcript = protocol_version || device_a || pub_a || device_b || pub_b || group_id
sas_bytes  = HKDF-SHA256(ikm = x25519(priv, peer_pub), salt = transcript, info = b"txtodo-sas-v1")
```

Getting this wrong is the single failure mode of the task, and it is invisible in testing — both
honest devices agree either way. Write the MITM test (below) or the guarantee is decorative.

Binding `protocol_version` into the transcript also kills downgrade: an attacker cannot talk v1 to
one side and v2 to the other without changing the words.

## Words

EFF short list, 1296 words (<https://www.eff.org/dice>, `eff_short_wordlist_1.txt`). Six words is
`6 × log2(1296) ≈ 62 bits` — plenty against an online attacker who gets one try inside the window.

- Vendor the list, checked in, with a test asserting 1296 entries, no duplicates, and a hash of the
  file. A wordlist that silently changes changes every SAS.
- Index by `sas_bytes` chunks; document the exact bit-slicing, because the two implementations that
  must agree are the same code today and a phone app at M9.

## Confirmation must be mutual

Both humans compare, both press yes. A one-sided confirm lets an attacker who controls the display
on one device complete the pairing. The group key is sent **after** both confirmations, encrypted
under a key derived from the same transcript (`info = b"txtodo-pair-v1"`, separate from the SAS
info string — never reuse one derived key for two purposes).

## The QR carries no secrets

`device`, `group_id`, the initiator's X25519 **public** key, the endpoint address, and a one-time
pairing nonce. Nothing in it is confidential; a photographed QR must be useless without the SAS
confirmation on the other end. `txtodo pair <code>` takes the same payload base32-encoded for people
without a camera.

## Bounds

- `PAIRING_WINDOW_MS` (leaning 120 000) after which the offer expires and the nonce is discarded.
- Single use: a nonce is consumed on first handshake, whether it succeeds or fails.
- `MAX_CONCURRENT_PAIRINGS`, asserted — one is the honest number; the cap stops a flood from
  holding ephemeral keys open.
- Rate-limit failed SAS confirmations, then close the window entirely rather than allow retries.

## Snapshot then ops

Once keyed, the new device gets a full snapshot (store `snapshots` table) and then the op tail. This
is the one path where a device legitimately holds no history, so it is also the easiest place to
accidentally accept an unsigned bulk import — verify every op in the snapshot's op range exactly
like a normal batch ([sync-crypto-envelope](../sync-crypto-envelope/notes.md)). Bound the snapshot
size and stream it; do not buffer a whole workspace in memory.

## Tests

- **MITM**: a relay that runs two handshakes and forwards. The two SAS strings must differ. This is
  the acceptance test for the whole task.
- Same transcript on both sides yields identical words; any single-bit change to any transcript
  field changes them.
- An expired nonce, a reused nonce, and a one-sided confirmation each fail with distinct errors and
  transfer no key.
- The QR payload, decoded, contains no key material — assert on the decoded struct's fields, so a
  future field addition has to be considered.
- Wordlist invariants (count, duplicates, file hash).
