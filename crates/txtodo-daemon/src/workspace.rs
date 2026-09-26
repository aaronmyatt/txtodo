//! The registry of documents and their actors for one workspace root. Discovery is the walker;
//! registration is idempotent so the startup walk and later directory-create rewalks share one
//! path. Removal is out of scope for M3 (an actor for a deleted file reconciles an empty file).
//!
//! Device id, sync group, keystore and pairing bookkeeping are no longer this struct's own fields
//! (ADR 0021, task `daemon-device-set-identity`): they live in a shared [`DeviceIdentity`],
//! borrowed via `identity: Arc<DeviceIdentity>` instead of minted per workspace. `open`/
//! `open_with_default_mode` (every test in this crate) mint their own throwaway, in-memory-keystore
//! identity internally, so no existing call site needs to change; `open_with_key_store`
//! (production) instead takes the catalog's one shared identity as a parameter. See
//! `device_identity.rs`'s module doc for the full rationale and migration story.

use crate::actor::{ActorConfig, FileActor, SharedStore};
use crate::clock::Clock;
use crate::device_identity::DeviceIdentity;
use crate::handle::{ActorError, ActorHandle};
use crate::lan_status::LanStatus;
use crate::notes_actor::NotesActorConfig;
use crate::notes_registry::{NotesCell, NotesRegistry};
use crate::pairing_lan_state::PairingLan;
use crate::pairing_state::PairingRegistry;
use crate::relay_state::RelayState;
use crate::stats::Stats;
use crate::tree_dirty::TreeDirty;
use crate::walker::{self, WALK_MAX_FILES, WalkError};
use crate::workspace_error::WorkspaceError;
use crate::workspace_mint::{basename, load_or_mint_identity_mode};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use txtodo_model::{DeviceId, FilePath, IdentityMode, WorkspaceTree};
use txtodo_store::{Store, WorkspaceId};
use txtodo_sync::{DeviceStaticPublic, GroupId, KeyStore};

/// Where the store lives under the workspace root (ADR 0010).
pub const STORE_FILE: &str = "oplog.db";

/// One workspace: root, shared store, shared device identity, Health counters and the live actors.
pub struct Workspace {
    root: PathBuf,
    store: SharedStore,
    clock: Arc<dyn Clock>,
    /// Device id, sync group, keystore and pairing registry — shared with every other workspace
    /// this daemon process has open (ADR 0021). See the module doc.
    identity: Arc<DeviceIdentity>,
    /// How every document in this workspace establishes task identity: minted once, at first
    /// open, and fixed for the workspace's lifetime (`load_or_mint_identity_mode`).
    pub(crate) identity_mode: IdentityMode,
    actors: BTreeMap<FilePath, ActorHandle>,
    started_at_ms: u64,
    stats: Arc<Stats>,
    /// Live `notes.md` actors, one per `ref:` directory, opened lazily (plan M5).
    notes: NotesRegistry,
    /// Marked by any actor whose commit could change the workspace tree; cleared by `tree.rs`'s
    /// `workspace_tree` after a rebuild (plan M5). `pub(crate)`: no accessor pair needed.
    pub(crate) tree_dirty: Arc<TreeDirty>,
    /// The last full rebuild of the workspace tree; stale exactly when `tree_dirty` is set.
    pub(crate) cached_tree: Mutex<WorkspaceTree>,
    /// Live LAN transport status (plan M4 `sync-lan-transport`), updated by `lan.rs`, read by
    /// `Health`/`txtodo doctor`.
    lan_status: LanStatus,
    /// Shared state connecting `lan.rs`'s background task to the pairing relay (`pairing_lan.rs`,
    /// plan M4 `sync-pairing`'s LAN wiring pass): the bound `LanEndpoint` and every raw mDNS
    /// sighting, regardless of sync group (see `pairing_lan_state.rs`'s module doc for why).
    pairing_lan: PairingLan,
    /// The bound relay endpoint, if any (plan M8 `sync-relay-enable`): set by `relay.rs`, read by
    /// `lan.rs`'s relay-fallback dial.
    relay_state: RelayState,
    /// This workspace's catalog identity, bound into every sealed batch's AEAD (stage 7 — see
    /// `set_workspace_id`'s doc). Freshly minted as a placeholder here since most tests in this
    /// crate open a `Workspace` with no registry at all.
    workspace_id: Mutex<WorkspaceId>,
    /// Where the root list and its ref dirs live (task workspace-layout), shared with every actor.
    layout: crate::layout_state::SharedLayout,
}

impl Workspace {
    /// `open_with_default_mode` under the project-wide default (`IdentityMode::Sidecar`,
    /// docs/questions.md Q2).
    pub fn open(root: &Path, clock: Arc<dyn Clock>) -> Result<Workspace, WorkspaceError> {
        Self::open_with_default_mode(root, clock, IdentityMode::Sidecar)
    }

    /// Opens the store under `<root>/.txtodo/`, mints (or reuses, if `<root>/.txtodo/identity.db`
    /// already exists from a prior run of this exact constructor) a throwaway, in-memory-keystore
    /// [`DeviceIdentity`] scoped to this one workspace, walks the tree and spawns one actor per
    /// document. Must run inside a tokio runtime (actors are tasks). `default_identity_mode`
    /// decides a brand-new workspace's mode when nothing on disk already carries an `id:` tag
    /// (`txtodod --identity-mode`); one already tagged is always `Tagged` regardless (plan
    /// decision 3). Every test in this crate uses this constructor (or `open`) precisely so none
    /// of them starts depending on OS keychain reachability; `open_with_key_store` is the real,
    /// persisted, shared-identity path (plan M4 `sync-keystore`, ADR 0021).
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
        let identity = Arc::new(DeviceIdentity::open_in_memory(&state_dir, clock.as_ref())?);
        Self::finish_open(root, clock, store, default_identity_mode, identity)
    }

    /// Production entry point (plan M4 `sync-keystore`, ADR 0021): `identity` is the one
    /// [`DeviceIdentity`] the catalog constructed for this whole daemon process and shares across
    /// every workspace it opens — never minted here. Kept as its own constructor rather than a
    /// change to `open`/`open_with_default_mode`: dozens of existing tests call those and must not
    /// start depending on OS keychain reachability in CI/sandboxes.
    pub fn open_with_key_store(
        root: &Path,
        clock: Arc<dyn Clock>,
        default_identity_mode: IdentityMode,
        identity: Arc<DeviceIdentity>,
    ) -> Result<Workspace, WorkspaceError> {
        let state_dir = root.join(walker::STATE_DIR);
        std::fs::create_dir_all(&state_dir).map_err(|source| WalkError::Io {
            path: state_dir.clone(),
            source,
        })?;
        let store = Store::open(&state_dir.join(STORE_FILE))?;
        Self::finish_open(root, clock, store, default_identity_mode, identity)
    }

    /// Shared tail of every constructor: loads or mints every workspace-lifetime id still local to
    /// this workspace (just `identity_mode` now — device/group/keystore live on `identity`), walks
    /// the tree and spawns one actor per document. Must run inside a tokio runtime (actors are
    /// tasks).
    fn finish_open(
        root: &Path,
        clock: Arc<dyn Clock>,
        mut store: Store,
        default_identity_mode: IdentityMode,
        identity: Arc<DeviceIdentity>,
    ) -> Result<Workspace, WorkspaceError> {
        let layout = crate::layout_file::initial(root);
        let extra = layout.custom_root_list().map(|p| root.join(p.as_str()));
        let identity_mode = load_or_mint_identity_mode(
            &mut store,
            (root, extra.as_deref()),
            default_identity_mode,
        )?;
        let started_at_ms = clock.now_ms();
        let placeholder_workspace_id = WorkspaceId::new(clock.new_ulid());
        let lan_status = identity.lan_status().clone();
        let pairing_lan = identity.pairing_lan().clone();
        let mut ws = Workspace {
            root: root.to_path_buf(),
            store: Arc::new(Mutex::new(store)),
            clock,
            identity,
            identity_mode,
            actors: BTreeMap::new(),
            started_at_ms,
            stats: Arc::new(Stats::default()),
            notes: NotesRegistry::new(),
            workspace_id: Mutex::new(placeholder_workspace_id),
            layout: crate::layout_state::SharedLayout::new(layout),
            tree_dirty: Arc::new(TreeDirty::default()),
            cached_tree: Mutex::new(WorkspaceTree::default()),
            lan_status,
            pairing_lan,
            relay_state: RelayState::default(),
        };
        ws.discover(root)?;
        debug_assert!(ws.actors.len() <= WALK_MAX_FILES);
        Ok(ws)
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
            device: self.device(),
            stats: Arc::clone(&self.stats),
            identity_mode: self.identity_mode,
            tree_dirty: Arc::clone(&self.tree_dirty),
            layout: self.layout.clone(),
        };
        let actor = FileActor::open(cfg, Arc::clone(&self.store), Arc::clone(&self.clock))
            .map_err(|e| WorkspaceError::Actor(path.clone(), Box::new(e)))?;
        self.actors.insert(path, actor.spawn());
        Ok(true)
    }

    /// The layout handle shared with every actor: where the root list's ref dirs live.
    pub fn layout(&self) -> &crate::layout_state::SharedLayout {
        &self.layout
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
    /// This device (shared across every workspace this daemon has open, ADR 0021).
    pub fn device(&self) -> DeviceId {
        self.identity.device()
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
    /// This device's sync group (plan M4 pairing, ADR 0021: shared across every open workspace).
    pub fn group(&self) -> GroupId {
        self.identity.group()
    }
    /// The keystore backing this device's sync keys (device signing/static, group key epochs) —
    /// shared across every workspace this daemon has open (ADR 0021).
    pub fn key_store(&self) -> &Arc<dyn KeyStore + Send + Sync> {
        self.identity.key_store()
    }
    /// The resolved sync keystore backend's name: `"memory"` for tests, `"os"`/`"file"` for real
    /// (plan M4 `sync-keystore`). `txtodo doctor` reports this by name.
    pub fn key_store_backend_name(&self) -> &'static str {
        self.identity.key_store_backend_name()
    }
    /// This device's long-term X25519 static public key, safe to hand to a peer during pairing.
    pub fn device_static_public(&self) -> DeviceStaticPublic {
        self.identity.device_static_public()
    }
    /// This device's persisted relay-transport identity seed (`relay.rs::start`'s bind call).
    pub fn relay_identity(&self) -> [u8; 32] {
        self.identity.relay_identity()
    }
    // This device's Ed25519 op-signing key is not a `Workspace` field: `bundle_grpc.rs` loads it
    // on demand from `key_store()` (plan M8 `cli-bundle`), the same "mint once, persist via the
    // keystore" idiom as `device_static_public` above but without adding a hot field for a code
    // path only two RPCs ever touch.
    /// The group key epoch this device currently seals ops under (plan M4 `sync-device-remove`,
    /// ADR 0021: shared across every open workspace); 0 until the first `device remove` rotates it.
    pub(crate) fn group_epoch(&self) -> u32 {
        self.identity.group_epoch()
    }
    /// Updates only the in-memory epoch; the caller (`device_remove.rs`, which already holds the
    /// identity store locked for the rest of the removal) is responsible for persisting
    /// `new_epoch` to `meta` itself under `device_identity::GROUP_EPOCH_KEY` first. Not a
    /// `StoreError`-returning method that re-locks `identity_store()` on its own: `Mutex` is not
    /// reentrant, and this is always called while `device_remove::remove_device` already holds
    /// that same lock — a prior version of this method deadlocked exactly that way.
    pub(crate) fn set_group_epoch(&self, new_epoch: u32) {
        self.identity.set_group_epoch(new_epoch);
    }
    /// This daemon's in-flight pairing bookkeeping (`pairing_grpc.rs`) — one attempt at a time,
    /// for the whole device, not per workspace (ADR 0021).
    pub(crate) fn pairing(&self) -> &PairingRegistry {
        self.identity.pairing()
    }
    /// Peers with a sync session open right now (task `sync-live-push`), device-wide.
    pub(crate) fn live_peers(&self) -> &crate::live_peers::LivePeers {
        self.identity.live_peers()
    }
    /// Which peers hold no key we share (task sync-drift line 5), device-wide.
    pub(crate) fn peer_keys(&self) -> &crate::peer_keys::PeerKeys {
        self.identity.peer_keys()
    }
    /// The device-global meta/devices rows (ADR 0021: `adopt_group_key`, `device_remove.rs`,
    /// `debug_hooks.rs`, `devices_grpc.rs` all go through this instead of `store()`).
    pub(crate) fn identity_store(&self) -> &Mutex<txtodo_store::IdentityStore> {
        self.identity.store()
    }
    /// The `notes.md` actor for `path`, opening it from disk/store on first use (plan M5). `path`
    /// need not already be a registered document — a notes document never gets a `FileActor`.
    pub(crate) fn notes_actor(&self, path: &FilePath) -> Result<NotesCell, ActorError> {
        let cfg = NotesActorConfig {
            path: path.clone(),
            disk: self.root.join(path.as_str()),
            device: self.device(),
        };
        self.notes.get_or_open(cfg, &self.store, &self.clock)
    }
    /// Records `device`'s relay reachability, learned from its `PairingOffer` (task
    /// `daemon-workspace-identity-agreement` stage 2) — a separate call from
    /// [`Workspace::adopt_group_key`] rather than a parameter on it, both to stay under the
    /// argument-count budget and because `Store::set_relay_reachability`'s own doc already makes
    /// relay reachability a distinct concern from registering a device at all.
    pub(crate) fn record_peer_relay_reachability(
        &self,
        device: DeviceId,
        relay_node_id: [u8; 32],
        relay_url: &str,
    ) -> Result<bool, txtodo_store::StoreError> {
        self.identity_store()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .set_relay_reachability(device, relay_node_id, relay_url)
    }
    /// Shared LAN-endpoint/sighting state the pairing relay driver (`pairing_lan.rs`) and `lan.rs`
    /// both need (plan M4 `sync-pairing`'s LAN wiring pass).
    pub(crate) fn pairing_lan(&self) -> &PairingLan {
        &self.pairing_lan
    }
    /// Live LAN transport status (plan M4 `sync-lan-transport`), for `Health`/`txtodo doctor`.
    pub fn lan_status(&self) -> &LanStatus {
        &self.lan_status
    }
    /// The bound relay endpoint, if any (plan M8 `sync-relay-enable`).
    pub(crate) fn relay_state(&self) -> &RelayState {
        &self.relay_state
    }
    /// Replaces this device's sync group id in memory (its persistence is the caller's job —
    /// `adopt_group_key`/`debug_hooks.rs`'s `debug_set_group_key` both write `meta` themselves
    /// first). Never called with the identity store not already updated to match.
    pub(crate) fn set_group(&self, group: GroupId) {
        self.identity.set_group(group);
    }
    /// This workspace's catalog identity, bound into every sealed batch's AEAD (stage 7). See the
    /// field's own doc: meaningful only once `set_workspace_id` replaces the placeholder.
    pub fn workspace_id(&self) -> WorkspaceId {
        *self
            .workspace_id
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }
    /// Replaces the placeholder `finish_open` minted with the real, catalog-assigned id. Called
    /// exactly once by `workspace_catalog_open.rs`, right after registering/opening — see the
    /// field's own doc for why the placeholder must never reach a peer before this runs.
    pub(crate) fn set_workspace_id(&self, id: WorkspaceId) {
        *self
            .workspace_id
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = id;
    }
}
