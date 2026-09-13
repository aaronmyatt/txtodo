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

## As built (2026-09-13, agent) — daemon wiring, `--key-store`, the passphrase prompt, `config.toml`

Reuses every backend/type from the previous pass unchanged (`OsKeyStore`, `FileKeyStore`,
`KeyStoreMode`, `ResolvedBackend`, `KeyStoreError`); nothing here re-derives a key or re-implements
an AEAD/KDF step.

- `crates/txtodo-daemon/src/keystore_setup.rs`: `resolve_key_store(state_dir, scope, mode,
  file_passphrase)` picks the backend per `mode`, scoped to the workspace's group id (so two
  groups on one machine never collide in the OS keystore) — `Workspace::open_with_key_store` is
  the new production entry point. **Judgement call, flagged for the human**: this does not call
  `txtodo_sync::resolve()` directly. That function's signature returns `Box<dyn KeyStore>` with no
  `Send + Sync` bound, but `Workspace` must be `Send + Sync` (it lives behind `Arc<RwLock<_>>`,
  driven from async gRPC handlers), and Rust has no safe way to add auto-trait bounds to an
  already-erased trait object. `resolve_key_store` reimplements only `resolve`'s three-line mode
  dispatch against the same concrete `OsKeyStore`/`FileKeyStore` types, never their internals.
- `Workspace::open_with_key_store` (`crates/txtodo-daemon/src/workspace.rs`) is a *new*
  constructor, not a change to `open`/`open_with_default_mode`. Those two (and therefore every
  existing test in this crate) keep the in-memory placeholder on purpose: dozens of tests construct
  a `Workspace` directly and must not start depending on OS keychain reachability in CI/sandboxes.
  `open_with_key_store` also mints and persists this device's long-term X25519 static key
  (`sync-device-remove`'s `DeviceStaticSecret`) under `KeyId::DeviceStatic` — the first time
  that key has lived anywhere but memory.
- `txtodod --key-store <auto|os|file>` (`crates/txtodo-daemon/src/main.rs`), mirroring
  `--identity-mode`'s own flag pattern. **Important asymmetry, and the one thing most worth a
  second look**: omitting the flag entirely is *not* the same as `--key-store auto`. Omitted keeps
  the exact pre-existing behaviour (the in-memory placeholder); `auto`/`os` only run when a human
  or service file opts in explicitly. First-pass version of this defaulted the omitted case to
  `auto`, which passed every test on this macOS sandbox (Keychain reachable) but would have broken
  every test across the workspace that spawns the real `txtodod` binary without a `--key-store`
  flag on a headless CI runner with no D-Bus Secret Service —
  `crates/txtodo-daemon/tests/{crash,editor_saves,external_edits,external_edits_sidecar}.rs` and
  `crates/txtodo-cli/tests/daemon_mode.rs` all do exactly that, and `.github/workflows/ci.yml`'s
  `daemon` job runs on `ubuntu-latest`. Caught by re-reading the CI matrix, not by a failing test
  (this sandbox has Keychain access, so nothing failed locally) — fixed before the first commit of
  this fact landed, and confirmed by manually running `txtodod --dir X` with and without the flag.
- `--key-store file`'s passphrase is read from this binary's own stdin (never a CLI argument or
  environment variable, CLAUDE.md §3.1) — `prompt_file_passphrase()` in `main.rs`. **Known gap,
  flagged for the human**: it does not suppress terminal echo. The task notes ask for "read without
  echo"; doing that needs a terminal-control dependency (e.g. `rpassword`) that was not added
  without sign-off, consistent with this repo's convention of flagging new dependencies rather than
  adding them speculatively (see `sync-lan-transport`'s `iroh`/`mdns-sd` additions for precedent).
  The two properties CLAUDE.md §3.1 actually requires — never a CLI arg, never an env var, never
  logged — hold regardless; the `String` read from stdin moves directly into the zeroizing `Secret`
  with no extra copy. Manually verified end to end: a piped passphrase creates `.txtodo/keystore`
  at mode `0600` and the daemon starts cleanly reporting `key_store=file`.
- `HealthResponse.key_store_backend` (new proto field) carries the resolved backend's name;
  `txtodo doctor` gained a sixth fixed check, `keystore`, that prints it — "a human should be able
  to answer where are my keys? without reading code," per the task's own framing.
- `config.toml`'s `key_store` field (`crates/txtodo-cli/src/config.rs`): a local `KeyStoreMode`
  enum mirroring `txtodo_sync::KeyStoreMode` (this crate cannot depend on `txtodo-sync`, same
  reason `IdentityMode` has its own local copy), `Config::key_store_mode()` (unset/unrecognised is
  `Auto`), reported by `txtodo env`. **Known, pre-existing limitation this does not fix**: like
  `identity_mode` before it, nothing threads this config value through to the daemon's own
  `--key-store` flag via the launchd/systemd service templates — a human sets it directly on
  `txtodod --key-store ...` (or the service file's `ExecStart` line) for now. This mirrors an
  already-existing gap for `--identity-mode`, not a new one introduced here.
- Tests: `crates/txtodo-cli/tests/daemon_mode.rs::doctor_reports_the_keystore_backend_and_no_peer_rows_when_unpaired`
  asserts the `keystore` row and its `backend: memory` value (the test daemon is spawned with no
  `--key-store` flag, exercising the exact "omitted never touches the OS keychain" path above).
  No test in this repo touches the real OS keychain (would pop a Keychain dialog in CI) — the
  `os`/`auto`-resolves-to-`os` path was verified once, manually, in this sandbox
  (`txtodod --dir X --key-store auto` reported `key_store=os`), not by an automated test, matching
  this crate's own stated rule that OS-keystore reachability is never asserted on in test code.
- `cargo build --workspace`, `cargo test -p txtodo-daemon -p txtodo-cli`, `cargo clippy --workspace
  --all-targets -D warnings`, `cargo fmt --all --check` all clean.
