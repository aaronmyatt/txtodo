//! The registry of documents and their actors for one workspace root. Discovery is the walker;
//! registration is idempotent so the startup walk and later directory-create rewalks share one
//! path. Removal is out of scope for M3 (an actor for a deleted file reconciles an empty file).

use crate::actor::{ActorConfig, FileActor, SharedStore};
use crate::clock::Clock;
use crate::handle::{ActorError, ActorHandle};
use crate::stats::Stats;
use crate::walker::{self, WALK_MAX_FILES, WalkError};
use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use txtodo_model::{DeviceId, FilePath, Ulid};
use txtodo_store::{Store, StoreError};

/// The `meta` key holding this install's device id.
pub const DEVICE_ID_KEY: &str = "device_id";
/// Where the store lives under the workspace root (ADR 0010).
pub const STORE_FILE: &str = "oplog.db";

/// Why the workspace could not open.
#[derive(Debug)]
pub enum WorkspaceError {
    /// The store failed.
    Store(StoreError),
    /// Discovery failed.
    Walk(WalkError),
    /// An actor failed to open.
    Actor(FilePath, Box<ActorError>),
    /// More documents than `WALK_MAX_FILES`.
    TooMany(usize),
}

impl fmt::Display for WorkspaceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WorkspaceError::Store(e) => write!(f, "store: {e}"),
            WorkspaceError::Walk(e) => write!(f, "discover documents: {e}"),
            WorkspaceError::Actor(p, e) => write!(f, "open {p}: {e}"),
            WorkspaceError::TooMany(n) => write!(f, "{n} documents, max {WALK_MAX_FILES}"),
        }
    }
}

impl std::error::Error for WorkspaceError {}

impl From<StoreError> for WorkspaceError {
    fn from(e: StoreError) -> WorkspaceError {
        WorkspaceError::Store(e)
    }
}
impl From<WalkError> for WorkspaceError {
    fn from(e: WalkError) -> WorkspaceError {
        WorkspaceError::Walk(e)
    }
}

/// One workspace: root, shared store, device id, Health counters and the live actors.
pub struct Workspace {
    root: PathBuf,
    store: SharedStore,
    clock: Arc<dyn Clock>,
    device: DeviceId,
    actors: BTreeMap<FilePath, ActorHandle>,
    started_at_ms: u64,
    stats: Arc<Stats>,
}

impl Workspace {
    /// Opens the store under `<root>/.txtodo/`, loads or mints the device id, walks the tree and
    /// spawns one actor per document. Must run inside a tokio runtime (actors are tasks).
    pub fn open(root: &Path, clock: Arc<dyn Clock>) -> Result<Workspace, WorkspaceError> {
        let state_dir = root.join(walker::STATE_DIR);
        std::fs::create_dir_all(&state_dir).map_err(|source| WalkError::Io {
            path: state_dir.clone(),
            source,
        })?;
        let mut store = Store::open(&state_dir.join(STORE_FILE))?;
        let device = load_or_mint_device(&mut store, clock.as_ref())?;
        let started_at_ms = clock.now_ms();
        let mut ws = Workspace {
            root: root.to_path_buf(),
            store: Arc::new(Mutex::new(store)),
            clock,
            device,
            actors: BTreeMap::new(),
            started_at_ms,
            stats: Arc::new(Stats::default()),
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

    /// Starts an actor for `path` unless one exists. Returns true when it started one.
    pub fn register(&mut self, path: FilePath) -> Result<bool, WorkspaceError> {
        if self.actors.contains_key(&path) {
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
    /// Shared Health counters.
    pub fn stats(&self) -> &Arc<Stats> {
        &self.stats
    }
    /// When the workspace opened, Unix ms.
    pub fn started_at_ms(&self) -> u64 {
        self.started_at_ms
    }
}

fn load_or_mint_device(store: &mut Store, clock: &dyn Clock) -> Result<DeviceId, StoreError> {
    if let Some(bytes) = store.meta_get(DEVICE_ID_KEY)?
        && let Ok(raw) = <[u8; 16]>::try_from(bytes.as_slice())
    {
        return Ok(DeviceId::new(Ulid::from_u128(u128::from_be_bytes(raw))));
    }
    let id = DeviceId::new(clock.new_ulid());
    store.meta_set(DEVICE_ID_KEY, &id.ulid().to_u128().to_be_bytes())?;
    debug_assert!(store.meta_get(DEVICE_ID_KEY)?.is_some());
    Ok(id)
}
