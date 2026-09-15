//! Resolves the real sync-keystore backend (plan M4 `sync-keystore`): OS keychain, encrypted
//! file, or the in-memory placeholder every test in this crate uses. Originally split out of
//! `workspace.rs` to keep that file within its line budget, the same reason `pairing_state.rs` is
//! split from `pairing_grpc.rs`; since ADR 0021 (task `daemon-device-set-identity`) the caller is
//! `device_identity.rs` — one device-global keystore, not one per workspace — but the resolution
//! logic itself is unchanged, just no longer scoped by a per-workspace group id (see
//! `resolve_key_store`'s doc).
//!
//! `txtodo_sync::resolve`'s own return type is `Box<dyn KeyStore>`, not `Send + Sync` — but
//! `DeviceIdentity` must be (it lives behind `Arc<_>`, driven from async gRPC handlers), so this
//! module reimplements `resolve`'s three-way dispatch directly against the concrete
//! `OsKeyStore`/`FileKeyStore` types (both real, unmodified `txtodo-sync` backends, not
//! reimplemented) rather than going through the generic helper's trait-object erasure.

use std::path::Path;
use std::sync::Arc;

use txtodo_sync::{
    DEVICE_STATIC_KEY_BYTES, DeviceSigningKey, DeviceStaticSecret, FileKeyStore, KeyId, KeyStore,
    KeyStoreError, KeyStoreMode, OsKeyStore, ResolvedBackend, SIGNING_KEY_BYTES, Secret,
};

use crate::workspace_error::WorkspaceError;

/// File name of the encrypted-file keystore backend, under whichever `state_dir`
/// `resolve_key_store` is called with (the device-global state dir since ADR 0021; only
/// `DeviceIdentity::open` ever calls this — `open_in_memory`, every test in this crate's own
/// entry point, never touches a real keystore backend at all) — never `$HOME` root (plan M4
/// `sync-keystore`'s own rule).
pub(crate) const KEYSTORE_FILE: &str = "keystore";

/// Resolves `mode` against `scope` (a fixed device-level string since ADR 0021 — one group per
/// device now, so no per-workspace-group scoping is needed to keep OS-keystore entries from
/// colliding), using `state_dir/KEYSTORE_FILE` for the file backend and `file_passphrase` to open
/// or create it. `auto` never falls back to a file on its own initiative
/// (`KeyStoreError::AutoNeedsChoice`) — the one rule the task notes call out by name. The passphrase
/// prompt itself lives in `txtodo-cli`; this function only consumes one if given.
pub(crate) fn resolve_key_store(
    state_dir: &Path,
    scope: &str,
    mode: KeyStoreMode,
    file_passphrase: Option<Secret>,
) -> Result<(Arc<dyn KeyStore + Send + Sync>, &'static str), WorkspaceError> {
    let keystore_path = state_dir.join(KEYSTORE_FILE);
    let open_file = move || -> Result<FileKeyStore, KeyStoreError> {
        let passphrase = file_passphrase.ok_or_else(|| KeyStoreError::Unavailable {
            backend: "file",
            reason: "key_store = \"file\" needs a passphrase; none was provided".to_owned(),
        })?;
        if keystore_path.exists() {
            FileKeyStore::open(&keystore_path, &passphrase)
        } else {
            FileKeyStore::create(&keystore_path, &passphrase)
        }
    };
    match mode {
        KeyStoreMode::Os => match OsKeyStore::probe(scope) {
            Ok(()) => Ok((
                Arc::new(OsKeyStore::new(scope.to_owned())),
                ResolvedBackend::Os.name(),
            )),
            Err(reason) => Err(KeyStoreError::Unavailable {
                backend: "os",
                reason,
            }
            .into()),
        },
        KeyStoreMode::File => {
            let store = open_file()?;
            Ok((Arc::new(store), ResolvedBackend::File.name()))
        }
        KeyStoreMode::Auto => match OsKeyStore::probe(scope) {
            Ok(()) => Ok((
                Arc::new(OsKeyStore::new(scope.to_owned())),
                ResolvedBackend::Os.name(),
            )),
            Err(reason) => Err(KeyStoreError::AutoNeedsChoice { reason }.into()),
        },
    }
}

/// Loads this device's long-term X25519 static keypair from the keystore, or mints and stores one
/// — the same "mint once, fixed for the workspace's lifetime" idiom as the device/group id.
pub(crate) fn load_or_mint_device_static(
    key_store: &dyn KeyStore,
) -> Result<DeviceStaticSecret, WorkspaceError> {
    if let Some(secret) = key_store.get(KeyId::DeviceStatic)? {
        let bytes: [u8; DEVICE_STATIC_KEY_BYTES] = secret
            .expose()
            .try_into()
            .map_err(|_| WorkspaceError::CorruptDeviceStatic(secret.expose().len()))?;
        return Ok(DeviceStaticSecret::from_bytes(bytes));
    }
    let generated = DeviceStaticSecret::generate();
    key_store.put(
        KeyId::DeviceStatic,
        &Secret::new(generated.to_bytes().to_vec()),
    )?;
    Ok(generated)
}

/// Loads this device's persisted relay-transport identity seed from the keystore, or mints and
/// stores one — same "mint once, fixed for the process's lifetime" idiom as
/// [`load_or_mint_device_static`], but the value is opaque bytes here: `txtodo_sync::holepunch`'s
/// `RelayEndpoint::bind_with_secret_key` is the only place these bytes become a real `iroh`
/// identity — this crate never names that type (`.claude/budgets.json`'s `allowedDeps`).
pub(crate) fn load_or_mint_relay_identity(
    key_store: &dyn KeyStore,
) -> Result<[u8; 32], WorkspaceError> {
    if let Some(secret) = key_store.get(KeyId::RelayIdentity)? {
        let bytes: [u8; 32] = secret
            .expose()
            .try_into()
            .map_err(|_| WorkspaceError::CorruptRelayIdentity(secret.expose().len()))?;
        return Ok(bytes);
    }
    let mut seed = [0u8; 32];
    getrandom::fill(&mut seed).map_err(|_| WorkspaceError::Entropy)?;
    key_store.put(KeyId::RelayIdentity, &Secret::new(seed.to_vec()))?;
    Ok(seed)
}

/// Loads this device's Ed25519 op-signing key from the keystore, or mints and stores one — same
/// "mint once, fixed for the workspace's lifetime" idiom as [`load_or_mint_device_static`]. Unlike
/// that one, `DeviceSigningKey` has no `to_bytes`/`generate` pair (it only wraps an existing
/// 32-byte seed), so the seed itself is minted here with the same injected-entropy call every
/// other id in this crate uses, then wrapped.
pub(crate) fn load_or_mint_device_signing(
    key_store: &dyn KeyStore,
) -> Result<DeviceSigningKey, WorkspaceError> {
    if let Some(secret) = key_store.get(KeyId::DeviceSigning)? {
        let bytes: [u8; SIGNING_KEY_BYTES] = secret
            .expose()
            .try_into()
            .map_err(|_| WorkspaceError::CorruptDeviceSigning(secret.expose().len()))?;
        return Ok(DeviceSigningKey::from_bytes(bytes));
    }
    let mut seed = [0u8; SIGNING_KEY_BYTES];
    getrandom::fill(&mut seed).map_err(|_| WorkspaceError::Entropy)?;
    key_store.put(KeyId::DeviceSigning, &Secret::new(seed.to_vec()))?;
    Ok(DeviceSigningKey::from_bytes(seed))
}
