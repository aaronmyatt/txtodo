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

## As built (2026-09-12, agent) — the `txtodo-sync` crypto/state-machine half only

- `crates/txtodo-sync/src/wordlists/eff_short_wordlist_1.txt`: the real EFF short wordlist (list 1,
  fetched from eff.org, dice-index column stripped), 1296 lines, no duplicates. Loaded via
  `eff_wordlist::wordlist()`; `WORDLIST_SHA256` pins the vendored file's own bytes.
- `crates/txtodo-sync/src/transcript.rs`: `transcript(protocol_version, Party, Party, GroupId)`.
  `Party { device, public_key }` replaces four positional args to stay under this workspace's
  5-argument limit. Order is canonical (ascending `DeviceId`), not initiator/joiner, so either side
  computes the same bytes from its own "self/peer" view without agreeing out of band on roles.
- `crates/txtodo-sync/src/sas.rs`: one `expand()` (HKDF-SHA256 extract-then-expand) feeding both
  `sas_words` (`info = "txtodo-sas-v1"`) and `pair_key` (`info = "txtodo-pair-v1"`), so the two
  outputs are provably from different `info` labels, never the same derived bytes reused. Each SAS
  word's index is a `u32` (not the literal bit-slice the notes sketch) read from its own 4-byte
  chunk of a `SAS_WORD_COUNT * 4`-byte expand, taken mod 1296 — documented bias (`≤ 1296 / 2^32`) in
  the doc comment rather than the more complex rejection-sampling that bias would otherwise call for.
- `crates/txtodo-sync/src/offer.rs`: `PairingOffer` (device, group, ephemeral X25519 public key,
  endpoint hint, nonce, **`issued_at_ms`**) plus postcard/base32 codecs. `issued_at_ms` is not in the
  notes' field list — added because the joiner has no local record of when the initiator issued the
  offer (see the nonce-registry judgement call below) and needs it to enforce `PAIRING_WINDOW_MS`
  independently. Still no secrets: an attacker forging the timestamp can only make an already
  nonce-bound, transcript-bound offer look older or newer, not extend it.
- `crates/txtodo-sync/src/nonce_registry.rs`: `PAIRING_WINDOW_MS` (120 000 ms), `MAX_CONCURRENT_PAIRINGS`
  (1, per the notes' own reading: "one is the honest number"). `NonceRegistry::issue`/`consume` is the
  initiator's own single-use/expiry bookkeeping on a nonce it minted; `witness` is a second method for
  the joiner, which never called `issue` on this nonce and so has no `open` record to check expiry
  against — it checks the offer's own `issued_at_ms` instead. Both maps are pruned (an unexpired-open
  cap plus a consumed-record retention window) so a long-running daemon's tables stay bounded.
- `crates/txtodo-sync/src/pairing.rs`: `PairingSession` — `offer`/`accept`/`complete` run the X25519
  ECDH and compute the transcript+SAS+pair-key eagerly; `confirm_local`/`confirm_remote`/`reject`
  track mutual confirmation; `is_ready_to_send_key` requires both flags and `!closed`;
  `wrap_group_key`/`unwrap_group_key` seal/open under `pair_key` with the transcript as AEAD
  associated data, refusing outside that state (`Closed` takes precedence over `NotConfirmed` when
  both would apply). `MAX_FAILED_SAS_CONFIRMATIONS = 3` closes the window on repeated mismatch,
  per "rate-limit, then close" rather than closing on the first one.
- `cargo deny check` run 2026-09-12: `advisories ok, bans ok, licenses ok, sources ok`.
  `x25519-dalek` 2.0.1, `hkdf` 0.12.4, `sha2` 0.10.9, `data-encoding` 2.11.1, `rand_core` 0.6.4 —
  all MIT/Apache-2.0.
- Judgement calls, flagged for the human:
  - Bit-slicing is 32-bit-chunk-then-modulo, not literal bit-slicing, and the resulting small bias
    is documented rather than eliminated by rejection sampling — flagged in case the human wants the
    stricter version.
  - `PairingOffer` gained `issued_at_ms` (see above); not in the notes' original field list.
  - The "static X25519 key per device, registered at pairing" mechanism `sync-device-remove` depends
    on is **not** built here: `PairingSession` only handles the ephemeral ECDH. Registering (and
    persisting, as ops, in a `devices` table) each device's long-term static key is unstarted —
    `sync-device-remove` cannot land until it does.
  - "Snapshot then op tail to the new device" is unstarted: it needs `txtodo-store`'s `snapshots`
    table wired to the wire protocol, which is daemon-adjacent integration work, not pure
    crypto/state-machine.
- Not in this slice: the `devices` table, static-key registration/persistence, snapshot+ops
  streaming to a newly paired device, and every `@cli` subtask (`txtodo pair`, the QR rendering, the
  mutual yes/no confirmation prompt). These land as their own commits under the one-slice-per-session
  rule; `tasks/sync-pairing/todo.txt` tracks which subtasks remain.
