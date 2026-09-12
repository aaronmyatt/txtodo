# txtodo-sync

## Purpose
Protocol, transports, pairing, crypto. Plan M4/M8.

## Public interface
- `Frame { version, body }` — the frozen envelope (`TXTO` · u16 LE version · u32 LE len · body);
  `Frame::{new, encode, decode, peek}`, `FrameError`, `PROTOCOL_VERSION`, `MAX_FRAME_BYTES`.
- `Message::{Hello, Want, Ops, Ack}` (append-only variants), `Message::{encode, decode,
  check_caps}`, `MessageError`, `Heads = BTreeMap<DeviceId, u64>`, `OriginRange`, `GroupId`,
  caps `MAX_OPS_PER_BATCH` / `MAX_WANT_RANGES` / `MAX_HEADS`. Goldens in `goldens/*.postcard`.
- `want(local, remote) -> Vec<OriginRange>`, `advance(heads, run) -> Result<(), Gap>`.
- `Session` — `Idle → Greeted → Wanting → Importing → (Wanting | Idle)` via `hello`, `on_hello`
  (group, protocol, `Skew` guard), `on_ops`, `committed`; `SessionError`, `Greeting`.
- Crypto: `sign(op, &DeviceSigningKey) -> Signature`, `verify(op, &Signature, &DevicePublicKey)`,
  `verify_batch(&[Op], &[Signature], &BTreeMap<DeviceId, DevicePublicKey>)` (all-or-nothing);
  `DeviceSigningKey`/`DevicePublicKey`/`Signature` with `from_bytes`/`to_bytes`.
- `seal(version, group, epoch, &GroupKey, plaintext)` / `open(version, group, &GroupKeys, sealed)`,
  `GroupKey`, `GroupKeys`, `CryptoError`, `MAX_RETAINED_KEY_EPOCHS`, header/AAD/nonce/tag byte consts.
- Keystore (M4 `sync-keystore`): `KeyStore` trait (`get`/`put`/`delete`), `KeyId`
  (`DeviceSigning`/`DeviceStatic`/`Group(epoch)`), `Secret` (redacted `Debug`, zeroized on drop),
  `KeyStoreError`, `MAX_STORED_EPOCHS`. Backends: `MemoryKeyStore` (tests only), `FileKeyStore`
  (`create`/`open`, Argon2id + XChaCha20-Poly1305, params in the file header,
  `ARGON2_MEMORY_KIB`/`ARGON2_ITERATIONS`/`ARGON2_PARALLELISM`), `OsKeyStore` (`keyring` crate,
  `probe` for reachability). `resolve(mode, probe_os, make_os, make_file) -> (ResolvedBackend, Box<dyn
  KeyStore>)` implements `key_store = "auto" | "os" | "file"`; `KeyStoreMode`, `ResolvedBackend::name`.
- Not here yet: transports, `pair` (later M4/M8 tasks).

## Invariants
- Every message versioned, authenticated, encrypted. Keys only in keystore.
- A device key and a group key are **injected**, never read here: `sign`/`seal` take them as arguments,
  so tests use fixtures and the keystore owns I/O. `seal` draws its nonce straight from `getrandom`;
  there is no RNG seam for the simulator's seeded PRNG to reach.
- Signature is per op and durable (`Op::signing_bytes` excludes nothing: the signature is never a
  field of `Op`); the AEAD is per batch and stripped at import. `verify_batch` is all-or-nothing.
- Sealed header is `version || group || epoch || nonce`, and `version || group || epoch` is the AAD,
  so a foreign group/version or a tampered header fails rather than decrypting into something
  plausible. An unknown epoch is a typed error naming it, never a try-every-key loop.
- Failure is validated, never asserted: one `CryptoError`, every variant names the epoch/device/group.
- `Frame`'s layout never changes; `Message` variants are only ever appended; a version we do not
  speak is a typed error that consumes nothing. Every wire collection has a cap checked on both
  paths; a hostile length is refused before allocation.
- `Ack` carries committed runs, never received ones; `advance` refuses a run that leaves a hole.
- Transport- and store-agnostic: no sockets, no SQLite; the caller moves frames and commits ops.
- `key_store = "auto"` never writes a key file on its own: an unreachable OS backend under `auto` is
  `KeyStoreError::AutoNeedsChoice`, not a silent fallback to `FileKeyStore`. No test in this crate
  calls the real `keyring` backend (would pop a Keychain dialog in CI); `resolve`'s OS-availability
  check is always injected, and `OsKeyStore` itself is exercised only by inspection, not by a test.
- `FileKeyStore` never `Debug`s or logs its derived key; a permissive file (group/world-readable) is
  refused, never `chmod`'d back. `create` refuses to replace an existing file (checked, then closed
  against the TOCTOU race with `hard_link` rather than `rename`, which would silently replace).
- May depend only on: txtodo-model, txtodo-store.
