//! Device-set identity (ADR 0021, task `daemon-device-set-identity`): one device id, sync group,
//! keystore and pairing registry shared by every workspace this daemon process opens, replacing
//! the pre-ADR-0021 shape where each `Workspace` minted/loaded its own (`workspace.rs:35-54`'s old
//! fields, `adopt_group_key` — see the ADR's own Context section for the exact lines this
//! supersedes). Constructed once per `txtodod` process (`main.rs::run`, before any workspace
//! opens) and threaded down through `WorkspaceOpenArgs` into every `Workspace::open_with_key_store`
//! call — never minted per workspace again.
//!
//! `state_dir` is the same directory that already holds `registry.db`/`txtodod.sock`/`txtodod.pid`
//! in whichever mode this process is running (`workspace_registry_paths::global_state_dir` for
//! true global mode, `<dir>/.txtodo/` for the legacy `--dir` bridge) — this file joins
//! `IDENTITY_DB_FILE` onto it, the same pattern `keystore_setup::resolve_key_store` already uses
//! for a workspace's own state dir. Bridge mode only ever opens one workspace per process anyway,
//! so scoping identity to that same directory (rather than reaching into the true-global location)
//! changes nothing observable and keeps every existing `--dir`-bridge test hermetic, exactly like
//! `registry.db` already is in that mode.
//!
//! **Migration story** (ADR 0021 Consequences: "amends ADR 0010... with a migration story for
//! existing per-workspace group keys"): there is no automatic adoption of any pre-existing
//! workspace's own group/keystore as the new device identity. A device identity is minted fresh
//! the first time this file is opened; a workspace that was already paired under the old
//! per-workspace scheme keeps its old group id/keystore sitting unused under its own
//! `.txtodo/oplog.db`/`.txtodo/keystore` (harmless, ignored, never deleted or migrated) and must be
//! re-paired under the new shared identity to sync again. This product has no released users yet
//! (M11 is still under active development) — a clean fresh mint is a simpler, more honest
//! migration story than guessing which of N pre-existing per-workspace groups should become "the"
//! device identity.

use std::path::Path;
use std::sync::{Arc, Mutex, PoisonError};

use crate::clock::Clock;
use crate::pairing_state::PairingRegistry;
use crate::walker::WalkError;
use crate::workspace_error::WorkspaceError;
use crate::workspace_offer_registry::WorkspaceOfferRegistry;
use txtodo_model::{DeviceId, Ulid};
use txtodo_store::{IdentityStore, StoreError};
use txtodo_sync::{
    DeviceStaticPublic, DeviceStaticSecret, GroupId, KeyStore, KeyStoreMode, MemoryKeyStore, Secret,
};

/// File name of the identity database, alongside `registry.db`/`txtodod.sock` at whichever state
/// dir this process resolved (see the module doc) — never under any single workspace's `.txtodo/`
/// in production, though test callers (`Workspace::open`/`open_with_default_mode`) do point
/// `state_dir` at one, for the same reason those constructors always use a real, if throwaway,
/// `Store` file rather than an in-memory one.
const IDENTITY_DB_FILE: &str = "identity.db";
/// The `meta` key holding this device's id.
const DEVICE_ID_KEY: &str = "device_id";
/// The `meta` key holding this device's sync group id (plan M4 pairing). `pub`, not `pub(crate)`:
/// `tests/support/mod.rs` (a separate integration-test crate) seeds a shared group id directly for
/// the real two-daemon LAN tests, the device-global analogue of `workspace_mint::GROUP_ID_KEY`.
pub const GROUP_ID_KEY: &str = "group_id";
/// The `meta` key holding the group key epoch this device currently seals ops under.
/// `pub(crate)`: `device_remove.rs` persists a rotation here.
pub(crate) const GROUP_EPOCH_KEY: &str = "group_key_epoch";
/// Fixed OS-keychain scope for the device identity: unlike the old per-workspace scheme (scoped by
/// group id, so N per-workspace groups on one machine never collided in one keychain), there is
/// now only one group per device, so no scoping by a value is needed at all.
const OS_KEYCHAIN_SCOPE: &str = "device";

/// One device id, sync group, keystore and pairing registry, shared by every workspace this daemon
/// process opens. See the module doc for where it lives on disk and its migration story.
pub struct DeviceIdentity {
    device: DeviceId,
    device_static: DeviceStaticSecret,
    group: Mutex<GroupId>,
    group_epoch: Mutex<u32>,
    key_store: Arc<dyn KeyStore + Send + Sync>,
    key_store_backend: &'static str,
    pairing: PairingRegistry,
    store: Mutex<IdentityStore>,
    relay_identity: [u8; 32],
    workspace_offers: WorkspaceOfferRegistry,
    /// Device-level since the LAN transport binds once per device (task `sync-live-push`): every
    /// `Workspace` holds a clone (both are `Arc`s inside), so `Health` on any workspace reads the
    /// one LAN task's progress and pairing sees every sighting.
    lan_status: crate::lan_status::LanStatus,
    pairing_lan: crate::pairing_lan_state::PairingLan,
    live_peers: crate::live_peers::LivePeers,
}

impl DeviceIdentity {
    /// Production entry point: resolves a real OS- or file-backed keystore per `key_store_mode`
    /// (plan M4 `sync-keystore`'s own resolver, reused as-is against a fixed device-level scope
    /// instead of a per-workspace group id). `defaulted` is true when `--key-store` was not given
    /// at all (`main.rs` then passes `KeyStoreMode::Auto` here to mean "try the OS keychain, but
    /// don't refuse to start over it") — see `keystore_setup::resolve_key_store`'s doc for what
    /// that changes.
    pub fn open(
        state_dir: &Path,
        clock: &dyn Clock,
        key_store_mode: KeyStoreMode,
        file_passphrase: Option<Secret>,
        defaulted: bool,
    ) -> Result<DeviceIdentity, WorkspaceError> {
        let store = open_store(state_dir)?;
        let (key_store, key_store_backend) = crate::keystore_setup::resolve_key_store(
            state_dir,
            OS_KEYCHAIN_SCOPE,
            key_store_mode,
            file_passphrase,
            defaulted,
        )?;
        Self::finish_open(store, clock, key_store, key_store_backend)
    }

    /// Test entry point: every test in this crate that opens a `Workspace` directly (`open`/
    /// `open_with_default_mode`) uses this instead of [`DeviceIdentity::open`], precisely so none
    /// of them starts depending on OS keychain reachability — the same reason
    /// `Workspace::open_with_default_mode` existed before this task. `state_dir` still gets a real
    /// `identity.db` (device/group ids persist for real); only the keystore is an in-memory
    /// placeholder.
    pub fn open_in_memory(
        state_dir: &Path,
        clock: &dyn Clock,
    ) -> Result<DeviceIdentity, WorkspaceError> {
        let store = open_store(state_dir)?;
        let key_store: Arc<dyn KeyStore + Send + Sync> = Arc::new(MemoryKeyStore::default());
        Self::finish_open(store, clock, key_store, "memory")
    }

    fn finish_open(
        mut store: IdentityStore,
        clock: &dyn Clock,
        key_store: Arc<dyn KeyStore + Send + Sync>,
        key_store_backend: &'static str,
    ) -> Result<DeviceIdentity, WorkspaceError> {
        let device = load_or_mint_device(&mut store, clock)?;
        let group = load_or_mint_group(&mut store)?;
        let group_epoch = load_group_epoch(&store)?;
        let device_static = crate::keystore_setup::load_or_mint_device_static(key_store.as_ref())?;
        let relay_identity =
            crate::keystore_setup::load_or_mint_relay_identity(key_store.as_ref())?;
        Ok(DeviceIdentity {
            device,
            device_static,
            group: Mutex::new(group),
            group_epoch: Mutex::new(group_epoch),
            key_store,
            key_store_backend,
            pairing: PairingRegistry::new(),
            store: Mutex::new(store),
            relay_identity,
            workspace_offers: WorkspaceOfferRegistry::new(),
            lan_status: crate::lan_status::LanStatus::default(),
            pairing_lan: crate::pairing_lan_state::PairingLan::default(),
            live_peers: crate::live_peers::LivePeers::default(),
        })
    }

    /// This device's id.
    pub fn device(&self) -> DeviceId {
        self.device
    }
    /// This device's long-term X25519 static public key, safe to hand to a peer during pairing.
    pub fn device_static_public(&self) -> DeviceStaticPublic {
        self.device_static.public_key()
    }
    /// This device's persisted relay-transport identity seed (task
    /// `daemon-workspace-identity-agreement` stage 1), stable across restarts. Opaque bytes: only
    /// `txtodo_sync::holepunch::RelayEndpoint::bind_with_secret_key` turns this into a real `iroh`
    /// identity — this crate never names that type.
    pub fn relay_identity(&self) -> [u8; 32] {
        self.relay_identity
    }
    /// Pending workspace offers this device has received but not yet accepted or declined (task
    /// `daemon-workspace-identity-agreement` stage 5) — the always-on control channel records
    /// into this; a gRPC surface (stage 6) lists and consumes from it.
    pub fn workspace_offers(&self) -> &WorkspaceOfferRegistry {
        &self.workspace_offers
    }
    /// The keystore backing this device's sync keys (device signing/static, group key epochs).
    pub fn key_store(&self) -> &Arc<dyn KeyStore + Send + Sync> {
        &self.key_store
    }
    /// The resolved sync keystore backend's name: `"memory"` for tests, `"os"`/`"file"` for real.
    pub fn key_store_backend_name(&self) -> &'static str {
        self.key_store_backend
    }
    /// This device's sync group: minted once, replaced with a peer's group once this device joins
    /// theirs (pairing).
    pub fn group(&self) -> GroupId {
        *self.group.lock().unwrap_or_else(PoisonError::into_inner)
    }
    /// Replaces this device's sync group id in memory; the caller is responsible for persisting it
    /// to `store()` first (the same discipline `Workspace::set_group` documented before this task).
    pub(crate) fn set_group(&self, group: GroupId) {
        *self.group.lock().unwrap_or_else(PoisonError::into_inner) = group;
    }
    /// The group key epoch this device currently seals ops under; 0 until the first `device
    /// remove` rotates it.
    pub(crate) fn group_epoch(&self) -> u32 {
        *self
            .group_epoch
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }
    /// Updates only the in-memory epoch; the caller (`device_remove.rs`) persists first.
    pub(crate) fn set_group_epoch(&self, new_epoch: u32) {
        *self
            .group_epoch
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = new_epoch;
    }
    /// This daemon's in-flight pairing bookkeeping — one attempt at a time, for the whole device,
    /// not per workspace (`pairing_grpc.rs`).
    pub(crate) fn pairing(&self) -> &PairingRegistry {
        &self.pairing
    }
    /// The device-global meta/devices rows (`adopt_group_key`, `device_remove.rs`,
    /// `debug_hooks.rs`, `devices_grpc.rs`).
    pub(crate) fn store(&self) -> &Mutex<IdentityStore> {
        &self.store
    }
    /// The device's LAN (and relay) status, shared by every workspace's `Health`.
    pub fn lan_status(&self) -> &crate::lan_status::LanStatus {
        &self.lan_status
    }
    /// Peers with a sync session open right now, whatever the carrier.
    pub(crate) fn live_peers(&self) -> &crate::live_peers::LivePeers {
        &self.live_peers
    }
    /// The bound LAN endpoint and every mDNS sighting, for pairing.
    pub(crate) fn pairing_lan(&self) -> &crate::pairing_lan_state::PairingLan {
        &self.pairing_lan
    }
}

fn open_store(state_dir: &Path) -> Result<IdentityStore, WorkspaceError> {
    std::fs::create_dir_all(state_dir).map_err(|source| WalkError::Io {
        path: state_dir.to_path_buf(),
        source,
    })?;
    Ok(IdentityStore::open(&state_dir.join(IDENTITY_DB_FILE))?)
}

fn load_or_mint_device(
    store: &mut IdentityStore,
    clock: &dyn Clock,
) -> Result<DeviceId, StoreError> {
    if let Some(bytes) = store.meta_get(DEVICE_ID_KEY)?
        && let Ok(raw) = <[u8; 16]>::try_from(bytes.as_slice())
    {
        return Ok(DeviceId::new(Ulid::from_u128(u128::from_be_bytes(raw))));
    }
    let id = DeviceId::new(clock.new_ulid());
    store.meta_set(DEVICE_ID_KEY, &id.ulid().to_u128().to_be_bytes())?;
    Ok(id)
}

/// Loads this device's sync group id from `meta`, or mints a fresh one (128 random bits) and
/// persists it — the device-global analogue of `workspace_mint::load_or_mint_group`.
fn load_or_mint_group(store: &mut IdentityStore) -> Result<GroupId, StoreError> {
    if let Some(bytes) = store.meta_get(GROUP_ID_KEY)?
        && let Ok(raw) = <[u8; 16]>::try_from(bytes.as_slice())
    {
        return Ok(GroupId(u128::from_be_bytes(raw)));
    }
    let mut raw = [0u8; 16];
    if getrandom::fill(&mut raw).is_err() {
        raw = [0xA5; 16];
    }
    store.meta_set(GROUP_ID_KEY, &raw)?;
    Ok(GroupId(u128::from_be_bytes(raw)))
}

/// Loads the group key epoch from `meta`, defaulting to 0 (no rotation has happened yet) — the
/// device-global analogue of `keystore_setup::load_group_epoch`.
fn load_group_epoch(store: &IdentityStore) -> Result<u32, StoreError> {
    let Some(bytes) = store.meta_get(GROUP_EPOCH_KEY)? else {
        return Ok(0);
    };
    let raw: [u8; 4] = bytes.as_slice().try_into().unwrap_or([0; 4]);
    Ok(u32::from_be_bytes(raw))
}
