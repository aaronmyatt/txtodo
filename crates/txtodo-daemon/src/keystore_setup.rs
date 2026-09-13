//! Resolves this workspace's real sync-keystore backend (plan M4 `sync-keystore`): OS keychain,
//! encrypted file, or the in-memory placeholder every test in this crate uses. Split out of
//! `workspace.rs` to keep that file within its line budget, the same reason `pairing_state.rs` is
//! split from `pairing_grpc.rs`.
//!
//! `txtodo_sync::resolve`'s own return type is `Box<dyn KeyStore>`, not `Send + Sync` — but
//! `Workspace` must be (it lives behind `Arc<RwLock<_>>`, driven from async gRPC handlers), so this
//! module reimplements `resolve`'s three-way dispatch directly against the concrete
//! `OsKeyStore`/`FileKeyStore` types (both real, unmodified `txtodo-sync` backends, not
//! reimplemented) rather than going through the generic helper's trait-object erasure.

use std::path::Path;
use std::sync::Arc;

use txtodo_store::{Store, StoreError};
use txtodo_sync::{
    DEVICE_STATIC_KEY_BYTES, DeviceSigningKey, DeviceStaticSecret, FileKeyStore, KeyId, KeyStore,
    KeyStoreError, KeyStoreMode, OsKeyStore, ResolvedBackend, SIGNING_KEY_BYTES, Secret,
};

use crate::workspace_error::WorkspaceError;

/// The `meta` key holding the group key epoch this workspace currently seals ops under (plan M4
/// `sync-device-remove`).
pub(crate) const GROUP_EPOCH_KEY: &str = "group_key_epoch";
/// File name of the encrypted-file keystore backend, under `<workspace>/.txtodo/` — never
/// `$HOME` root (plan M4 `sync-keystore`'s own rule).
pub(crate) const KEYSTORE_FILE: &str = "keystore";

/// Resolves `mode` against `scope` (this workspace's group id, so two groups on one machine never
/// share an OS-keystore entry), using `state_dir/KEYSTORE_FILE` for the file backend and
/// `file_passphrase` to open or create it. `auto` never falls back to a file on its own initiative
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

/// Loads the group key epoch from `meta`, defaulting to 0 (no rotation has happened yet). Unlike
/// the device/group id, 0 is never persisted just for being read: `Workspace::advance_group_epoch`
/// is the only writer, so an unrotated workspace's `meta` carries no `GROUP_EPOCH_KEY` at all.
pub(crate) fn load_group_epoch(store: &Store) -> Result<u32, StoreError> {
    let Some(bytes) = store.meta_get(GROUP_EPOCH_KEY)? else {
        return Ok(0);
    };
    let raw: [u8; 4] = bytes.as_slice().try_into().unwrap_or([0; 4]);
    Ok(u32::from_be_bytes(raw))
}
