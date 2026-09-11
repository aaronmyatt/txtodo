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
