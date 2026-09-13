//! The registry of documents and their actors for one workspace root. Discovery is the walker;
//! registration is idempotent so the startup walk and later directory-create rewalks share one
//! path. Removal is out of scope for M3 (an actor for a deleted file reconciles an empty file).

use crate::actor::{ActorConfig, FileActor, SharedStore};
use crate::clock::Clock;
use crate::handle::{ActorError, ActorHandle};
use crate::notes_actor::NotesActorConfig;
use crate::notes_registry::{NotesCell, NotesRegistry};
use crate::pairing_state::{PairingRegistry, PairingStateError};
use crate::stats::Stats;
use crate::walker::{self, WALK_MAX_FILES, WalkError};
use crate::workspace_error::WorkspaceError;
use crate::workspace_mint::{
    GROUP_ID_KEY, basename, load_or_mint_device, load_or_mint_group, load_or_mint_identity_mode,
};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use txtodo_model::{DeviceId, FilePath, IdentityMode};
use txtodo_store::{NewDevice, Store};
use txtodo_sync::{
    DeviceStaticPublic, DeviceStaticSecret, GroupId, KeyStore, KeyStoreMode, MemoryKeyStore, Secret,
};

/// Where the store lives under the workspace root (ADR 0010).
pub const STORE_FILE: &str = "oplog.db";

/// One workspace: root, shared store, device id, Health counters and the live actors.
pub struct Workspace {
    root: PathBuf,
    store: SharedStore,
    clock: Arc<dyn Clock>,
    device: DeviceId,
    /// How every document in this workspace establishes task identity: minted once, at first
    /// open, and fixed for the workspace's lifetime (`load_or_mint_identity_mode`).
    identity_mode: IdentityMode,
    actors: BTreeMap<FilePath, ActorHandle>,
    started_at_ms: u64,
    stats: Arc<Stats>,
    /// This workspace's sync group (plan M4 pairing): minted once, like the device id, and
    /// replaced with a peer's group once this device joins theirs — see [`Workspace::adopt_group_key`].
    group: Mutex<GroupId>,
    /// Where this workspace's sync keys live (device/group/device-static). `Workspace::open`/
    /// `open_with_default_mode` (every test in this crate) use an in-memory placeholder, not
    /// persisted across restarts; `Workspace::open_with_key_store` (production, `txtodod`) resolves
    /// a real OS/file-backed store (plan M4 `sync-keystore`).
    key_store: Arc<dyn KeyStore + Send + Sync>,
    /// The resolved backend's name (`"memory"`/`"os"`/`"file"`), so `txtodo doctor` can report it
    /// without reading code (plan M4 `sync-keystore`).
    key_store_backend: &'static str,
    /// This device's long-term X25519 static keypair (plan M4 `sync-device-remove`): minted once,
    /// like the device/group id, and stored via the keystore under `KeyId::DeviceStatic` so a
    /// future rotation can be handed to a remaining device even while it is offline.
    device_static: DeviceStaticSecret,
    /// The group key epoch this workspace currently seals ops under: 0 until the first `device
    /// remove` rotates it. Persisted in `meta` so a rotation survives a restart.
    group_epoch: Mutex<u32>,
    /// This daemon's in-flight pairing bookkeeping (`pairing_grpc.rs`).
    pairing: PairingRegistry,
    /// Live `notes.md` actors, one per `ref:` directory, opened lazily (plan M5).
    notes: NotesRegistry,
}

impl Workspace {
    /// `open_with_default_mode` under the project-wide default (`IdentityMode::Sidecar`,
    /// docs/questions.md Q2).
    pub fn open(root: &Path, clock: Arc<dyn Clock>) -> Result<Workspace, WorkspaceError> {
        Self::open_with_default_mode(root, clock, IdentityMode::Sidecar)
    }

    /// Opens the store under `<root>/.txtodo/`, loads or mints the device id, walks the tree and
    /// spawns one actor per document. Must run inside a tokio runtime (actors are tasks).
    /// `default_identity_mode` decides a brand-new workspace's mode when nothing on disk already
    /// carries an `id:` tag (`txtodod --identity-mode`); one already tagged is always `Tagged`
    /// regardless (plan decision 3).
    pub fn open_with_default_mode(
        root: &Path,
        clock: Arc<dyn Clock>,
        default_identity_mode: IdentityMode,
    ) -> Result<Workspace, WorkspaceError> {
        let state_dir = root.join(walker::STATE_DIR);
        std::fs::create_dir_all(&state_dir).map_err(|source| WalkError::Io {
            path: state_dir.clone(),
            source,
        })?;
        let store = Store::open(&state_dir.join(STORE_FILE))?;
        // Placeholder backend: not persisted across restarts. Every test in this crate uses this
        // constructor (or `open`) precisely so none of them starts depending on OS keychain
        // reachability; `open_with_key_store` is the real, persisted path (plan M4 `sync-keystore`).
        Self::finish_open(
            root,
            clock,
            store,
            default_identity_mode,
            (Arc::new(MemoryKeyStore::default()), "memory"),
        )
    }

    /// Production entry point (plan M4 `sync-keystore`): resolves a real OS- or file-backed
    /// keystore per `key_store_mode` instead of the in-memory placeholder `open`/
    /// `open_with_default_mode` use. Kept as its own constructor rather than a change to those two:
    /// dozens of existing tests call them and must not start depending on OS keychain reachability
    /// in CI/sandboxes. `file_passphrase` is required (and used) only when `key_store_mode`
    /// resolves to `File` — the human-facing prompt for it lives in `txtodo-cli`/`main.rs`, never
    /// here (this module has no notion of a terminal).
    pub fn open_with_key_store(
        root: &Path,
        clock: Arc<dyn Clock>,
        default_identity_mode: IdentityMode,
        key_store_mode: KeyStoreMode,
        file_passphrase: Option<Secret>,
    ) -> Result<Workspace, WorkspaceError> {
        let state_dir = root.join(walker::STATE_DIR);
        std::fs::create_dir_all(&state_dir).map_err(|source| WalkError::Io {
            path: state_dir.clone(),
            source,
        })?;
        let mut store = Store::open(&state_dir.join(STORE_FILE))?;
        // The group id scopes the OS-keystore entries so two groups on one machine never collide;
        // `finish_open` below loads it again (idempotent) once it also has the device/identity ids.
        let scope = load_or_mint_group(&mut store)?.0.to_string();
        let resolved = crate::keystore_setup::resolve_key_store(
            &state_dir,
            &scope,
            key_store_mode,
            file_passphrase,
        )?;
        Self::finish_open(root, clock, store, default_identity_mode, resolved)
    }

    /// Shared tail of every constructor: loads or mints every workspace-lifetime id, walks the
    /// tree and spawns one actor per document. Must run inside a tokio runtime (actors are tasks).
    /// `default_identity_mode` decides a brand-new workspace's mode when nothing on disk already
    /// carries an `id:` tag (`txtodod --identity-mode`); one already tagged is always `Tagged`
    /// regardless (plan decision 3). `key_store` bundles the backend and its reported name so this
    /// function stays under the arg-count budget.
    fn finish_open(
        root: &Path,
        clock: Arc<dyn Clock>,
        mut store: Store,
        default_identity_mode: IdentityMode,
        key_store: (Arc<dyn KeyStore + Send + Sync>, &'static str),
    ) -> Result<Workspace, WorkspaceError> {
        let (key_store, key_store_backend) = key_store;
        let device = load_or_mint_device(&mut store, clock.as_ref())?;
        let group = load_or_mint_group(&mut store)?;
        let identity_mode = load_or_mint_identity_mode(&mut store, root, default_identity_mode)?;
        let device_static = crate::keystore_setup::load_or_mint_device_static(key_store.as_ref())?;
        let group_epoch = crate::keystore_setup::load_group_epoch(&store)?;
        let started_at_ms = clock.now_ms();
        let mut ws = Workspace {
            root: root.to_path_buf(),
            store: Arc::new(Mutex::new(store)),
            clock,
            device,
            identity_mode,
            actors: BTreeMap::new(),
            started_at_ms,
            stats: Arc::new(Stats::default()),
            group: Mutex::new(group),
            key_store,
            key_store_backend,
            device_static,
            group_epoch: Mutex::new(group_epoch),
            pairing: PairingRegistry::new(),
            notes: NotesRegistry::new(),
        };
        ws.discover(root)?;
        debug_assert!(ws.actors.len() <= WALK_MAX_FILES);
        Ok(ws)
    }

    /// Walks `dir` (the root or a newly created subdirectory) and registers every document found.
    /// Returns how many actors were started.
    pub fn discover(&mut self, dir: &Path) -> Result<usize, WorkspaceError> {
        debug_assert!(
            dir.starts_with(&self.root),
            "discover stays inside the workspace"
        );
        let mut started = 0usize;
        for rel in walker::walk(dir)? {
            let abs = dir.join(rel.as_str());
            let path = walker::relative(&self.root, &abs)?;
            if self.register(path)? {
                started += 1;
            }
        }
        Ok(started)
    }

    /// Starts an actor for `path` unless one exists. Returns true when it started one. `notes.md`
    /// is discovered (`walker::is_notes_document`) but never gets a `FileActor` here: it is Loro
    /// text prose, not task lines, and a future notes actor (tasks/crdt-notes-doc) owns it — see
    /// `walker.rs`'s module doc and `workspace_tests::notes_md_is_left_alone`.
    pub fn register(&mut self, path: FilePath) -> Result<bool, WorkspaceError> {
        if self.actors.contains_key(&path) || walker::is_notes_document(basename(&path)) {
            return Ok(false);
        }
        if self.actors.len() >= WALK_MAX_FILES {
            return Err(WorkspaceError::TooMany(self.actors.len() + 1));
        }
        let cfg = ActorConfig {
            path: path.clone(),
            disk: self.root.join(path.as_str()),
            device: self.device,
            stats: Arc::clone(&self.stats),
            identity_mode: self.identity_mode,
        };
        let actor = FileActor::open(cfg, Arc::clone(&self.store), Arc::clone(&self.clock))
            .map_err(|e| WorkspaceError::Actor(path.clone(), Box::new(e)))?;
        self.actors.insert(path, actor.spawn());
        Ok(true)
    }

    /// The actor for a document, if registered.
    pub fn actor(&self, path: &FilePath) -> Option<&ActorHandle> {
        self.actors.get(path)
    }

    /// Every registered document, sorted.
    pub fn paths(&self) -> impl Iterator<Item = &FilePath> {
        self.actors.keys()
    }

    /// The actor for the document at absolute `disk` path, if it is a registered document.
    pub fn actor_for_disk(&self, disk: &Path) -> Option<&ActorHandle> {
        let rel = walker::relative(&self.root, disk).ok()?;
        self.actors.get(&rel)
    }

    /// The workspace root.
    pub fn root(&self) -> &Path {
        &self.root
    }
    /// The shared store (History RPC, doctor).
    pub fn store(&self) -> &SharedStore {
        &self.store
    }
    /// This device.
    pub fn device(&self) -> DeviceId {
        self.device
    }
    /// How every document in this workspace establishes task identity (docs/questions.md Q2).
    pub fn identity_mode(&self) -> IdentityMode {
        self.identity_mode
    }
    /// The injected clock: entropy and time enter the daemon only through this (plan §5's
    /// "inject the clock" idiom), so a token's id and timestamps come from here, never a bare
    /// `SystemTime`/`getrandom` call at the RPC boundary.
    pub fn clock(&self) -> &Arc<dyn Clock> {
        &self.clock
    }
    /// Shared Health counters.
    pub fn stats(&self) -> &Arc<Stats> {
        &self.stats
    }
    /// When the workspace opened, Unix ms.
    pub fn started_at_ms(&self) -> u64 {
        self.started_at_ms
    }
    /// This workspace's sync group (plan M4 pairing).
    pub fn group(&self) -> GroupId {
        *self
            .group
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
    /// The keystore backing this workspace's sync keys (device signing/static, group key epochs).
    /// See the module doc on [`load_or_mint_group`] for the current placeholder backend.
    pub fn key_store(&self) -> &Arc<dyn KeyStore + Send + Sync> {
        &self.key_store
    }
    /// The resolved sync keystore backend's name (plan M4 `sync-keystore`): `"memory"` for every
    /// test and the `open`/`open_with_default_mode` convenience constructors, `"os"`/`"file"` for
    /// a real backend resolved by `open_with_key_store`. `txtodo doctor` reports this by name.
    pub fn key_store_backend_name(&self) -> &'static str {
        self.key_store_backend
    }
    /// This device's long-term X25519 static public key (plan M4 `sync-device-remove`), safe to
    /// hand to a peer during pairing.
    pub fn device_static_public(&self) -> DeviceStaticPublic {
        self.device_static.public_key()
    }
    /// The group key epoch this workspace currently seals ops under (plan M4
    /// `sync-device-remove`); 0 until the first `device remove` rotates it.
    pub(crate) fn group_epoch(&self) -> u32 {
        *self
            .group_epoch
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }
    /// Updates only the in-memory epoch; the caller (`device_remove.rs`, which already holds the
    /// store locked for the rest of the removal) is responsible for persisting `new_epoch` to
    /// `meta` itself under `keystore_setup::GROUP_EPOCH_KEY` first. Not a `StoreError`-returning
    /// method that re-locks `self.store()` on its own: `Mutex` is not reentrant, and this is
    /// always called while `device_remove::remove_device` already holds that same lock — a
    /// prior version of this method deadlocked exactly that way.
    pub(crate) fn set_group_epoch(&self, new_epoch: u32) {
        *self
            .group_epoch
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = new_epoch;
    }
    /// This daemon's in-flight pairing bookkeeping (`pairing_grpc.rs`).
    pub(crate) fn pairing(&self) -> &PairingRegistry {
        &self.pairing
    }
    /// The `notes.md` actor for `path`, opening it from disk/store on first use (plan M5). `path`
    /// need not already be a registered document — a notes document never gets a `FileActor`.
    pub(crate) fn notes_actor(&self, path: &FilePath) -> Result<NotesCell, ActorError> {
        let cfg = NotesActorConfig {
            path: path.clone(),
            disk: self.root.join(path.as_str()),
            device: self.device,
        };
        self.notes.get_or_open(cfg, &self.store, &self.clock)
    }
    /// Finishes a pairing on the joiner's side: unwraps the sealed [`txtodo_sync::PairingGrant`]
    /// the initiator sent (see `pairing_grpc.rs`'s module doc — there is no transport yet, so this
    /// is driven directly by whoever stands in for one today), stores the group key under this
    /// workspace's keystore, registers the initiator's long-term static public key in the
    /// `devices` table (plan M4 `sync-device-remove` — this is the leg of the exchange this daemon
    /// wires; see `pairing_state.rs::adopt_group_key`'s doc for the direction it does not), and
    /// adopts `group` as this workspace's own — atomically from the caller's point of view, so
    /// this workspace never claims a group it does not also hold the key for. Only called by
    /// `pairing_grpc_tests.rs` today (no transport exists to call it in production yet).
    #[allow(dead_code)]
    pub(crate) fn adopt_group_key(
        &self,
        group: GroupId,
        sealed: &[u8],
        now_ms: u64,
    ) -> Result<(), PairingStateError> {
        let (peer_device, peer_static) =
            self.pairing
                .adopt_group_key(self.key_store.as_ref(), sealed, now_ms)?;
        let mut store = self
            .store
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        store.meta_set(GROUP_ID_KEY, &group.0.to_be_bytes())?;
        store.register_device(&NewDevice {
            device: peer_device,
            name: String::new(),
            static_public: peer_static.to_bytes(),
            paired_at_ms: now_ms,
            last_known_wall_ms: None,
            key_epoch: 0,
        })?;
        drop(store);
        *self
            .group
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = group;
        Ok(())
    }
}
