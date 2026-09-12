# Keys in the OS keystore via `keyring`, encrypted-file passphrase fallback on headless Linux — M4

Everything that needs a key gets it from here: the device Ed25519 signing key
([sync-crypto-envelope](../sync-crypto-envelope/notes.md)), the device's long-term X25519 key
([sync-device-remove](../sync-device-remove/notes.md)), and the group key per epoch. Ref:
<https://docs.rs/keyring> — macOS Keychain, Windows Credential Manager, Linux Secret Service over
D-Bus.

## The fallback must never happen silently

"Falls back to an encrypted file on headless Linux" is one sentence away from "falls back to an
encrypted file whenever the Keychain is locked, the D-Bus session is missing, or the user is on a
VPN that broke the session bus". That is a downgrade the user never agreed to: their keys move from
an OS-protected store to a file in their home directory, and nothing tells them.

So the backend is **chosen, not discovered**:

```toml
# config.toml
key_store = "auto"   # auto | os | file
```

- `os` — OS keystore only. Unavailable is a hard error with the reason.
- `file` — encrypted file only, chosen deliberately.
- `auto` — try `os`; if unavailable, **stop and print what to do**, and only use `file` once the
  user has set it. `auto` never writes a key file on its own.

`txtodo doctor` reports the active backend by name. A human should be able to answer "where are my
keys?" without reading code.

## The file backend

- Passphrase → key via Argon2id (<https://docs.rs/argon2>, RFC 9106
  <https://www.rfc-editor.org/rfc/rfc9106>). Parameters are named constants with units
  (`ARGON2_MEMORY_KIB`, `ARGON2_ITERATIONS`, `ARGON2_PARALLELISM`) and are stored in the file
  header, so raising them later still opens old files.
- Contents sealed with XChaCha20-Poly1305, the same primitive as the wire — one AEAD in the
  codebase, not two.
- Mode `0600`, created with those permissions, not chmod'd after. Refuse to open a file that is
  group- or world-readable rather than "fixing" it — a permissive file may already have been read.
- Path under `<workspace>/.txtodo/` alongside the rest of the state, never `$HOME` root.

## Shape

```rust
pub trait KeyStore {
    fn get(&self, id: KeyId) -> Result<Option<Secret>, KeyStoreError>;
    fn put(&self, id: KeyId, secret: &Secret) -> Result<(), KeyStoreError>;
    fn delete(&self, id: KeyId) -> Result<(), KeyStoreError>;
}
```

`KeyId` is an enum (`DeviceSigning`, `DeviceStatic`, `Group(epoch)`) — not a string, so a typo
cannot silently create a second key. Tests use an in-memory implementation; no test ever touches
the real Keychain, which also keeps CI from popping a dialog on macOS.

## Handling the secrets themselves

- `Secret` wraps the bytes, implements `Drop` via `zeroize` (<https://docs.rs/zeroize>), and has
  **no** `Debug`/`Display` that prints contents — a manual `Debug` writing `Secret(<redacted>)`.
  Derived `Debug` on a key type is how keys end up in logs and in `tracing` spans.
- Never a key in an error message. `KeyStoreError` names the `KeyId` and the backend, never bytes.
- Never a key as a CLI argument or env var (CLAUDE.md §3.1). Passphrase is prompted, read without
  echo, and zeroized.

## Bounds

`MAX_STORED_EPOCHS` mirrors `MAX_RETAINED_KEY_EPOCHS` from the crypto task — the same number in two
places is a drift bug waiting to happen, so it is defined once and imported. Keystore entry sizes
are small on every backend, but a group with many epochs is a growing collection: one entry per
epoch, capped, rather than one ever-growing blob.

## Tests

- `auto` with the OS backend unavailable errors and writes nothing — assert no file appears.
- A group/world-readable key file is refused, not repaired.
- Argon2 parameters round-trip from the header; a file written with low parameters still opens after
  the constants are raised.
- `Secret`'s `Debug` output contains no key bytes (cheap, and it catches the derive being added
  back).
- Round-trip through the in-memory store for every `KeyId` variant, exhaustively.

## As built (2026-09-12, agent) — the `txtodo-sync` half only

- `crates/txtodo-sync/src/keystore.rs`: `KeyId` (`DeviceSigning`/`DeviceStatic`/`Group(u32)`, `Ord`
  so a `BTreeMap<KeyId, _>` never touches a hash-ordered collection), `Secret` (owns the bytes,
  zeroized on `Drop`, hand-written `Debug` prints `Secret(<redacted>)`), the `KeyStore` trait
  (`get`/`put`/`delete`), `MAX_STORED_EPOCHS` — a re-export of
  [`sync-crypto-envelope`](../sync-crypto-envelope/notes.md)'s `MAX_RETAINED_KEY_EPOCHS`, not a
  second constant.
- `crates/txtodo-sync/src/keystore_memory.rs`: `MemoryKeyStore`, a `Mutex<BTreeMap>` for tests.
- `crates/txtodo-sync/src/keystore_file.rs`: `FileKeyStore`. Layout `magic || version ||
  memory_kib || iterations || parallelism || salt(16) || nonce(24) || ciphertext`; everything up to
  the nonce is both the clear header and the AEAD associated data. Argon2id params travel in the
  header, read back on `open`, so raising `ARGON2_MEMORY_KIB`/`ITERATIONS`/`PARALLELISM` never
  strands an old file (tested). Permission check runs before every read, refusing (not repairing) a
  group/world-readable file. `create` refuses to replace an existing file — checked, then closed
  against the TOCTOU race with `hard_link` (fails atomically if the target exists) rather than
  `rename` (which would silently replace it); `put`/`delete` still use temp-file-then-`rename` since
  those are legitimate replacements. Both paths write the temp file at mode `0600` directly, never
  `chmod`'d after.
- `crates/txtodo-sync/src/keystore_os.rs`: `OsKeyStore` over the `keyring` crate, one entry per
  `(scope, KeyId)` so two groups on one machine cannot collide. `probe` round-trips a throwaway entry
  to test reachability without assuming any real key exists yet.
- `crates/txtodo-sync/src/keystore_resolve.rs`: `resolve(mode, probe_os, make_os, make_file)`. The
  one rule that matters: `KeyStoreMode::Auto` with the OS probe failing returns
  `KeyStoreError::AutoNeedsChoice` and never calls `make_file` — no file is written on `auto`'s own
  initiative. `probe_os` is always an injected closure, in production `|| OsKeyStore::probe(scope)`;
  no test in this crate touches the real `keyring` backend (would pop a Keychain dialog in CI).
- `cargo deny check` run 2026-09-12: `advisories ok, bans ok, licenses ok, sources ok`. `keyring`
  3.6.3 (MIT/Apache-2.0), `argon2` 0.5.3 (MIT/Apache-2.0), `zeroize` 1.9.0 (MIT/Apache-2.0).
- Judgement calls, flagged for the human:
  - `KeyId` needed `Serialize`/`Deserialize` (the file backend's on-disk map is a
    `BTreeMap<KeyId, Vec<u8>>`, postcard-encoded) — added the derives rather than hand-rolling a
    second encoding. `Secret` itself stays non-`Serialize`; the file backend copies
    `secret.expose()` into a plain `Vec<u8>` for the map instead.
  - `OsKeyStore` and its `keyring`-backed `probe` have no dedicated unit test, per the task's own
    "no test ever touches the real Keychain" rule — the `auto`/`os`/`file` decision logic they feed
    is exercised in full via injected fakes in `keystore_resolve_tests.rs`.
- Not in this slice (the three `@cli` subtasks below, and the parent line): `config.toml`'s
  `key_store` field, prompting the passphrase without echo, and `txtodo doctor` printing the active
  backend by name (`ResolvedBackend::name()` is ready for it). These are `txtodo-cli` changes and
  land as their own commit under this repo's one-slice-per-session rule.
