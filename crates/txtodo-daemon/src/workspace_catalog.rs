//! The set of workspaces this one `txtodod` process currently has open (ADR 0025, task
//! `daemon-global-socket`): wraps `WorkspaceRegistry` (the catalog of known directories, task
//! `daemon-workspace-registry`) with live `Workspace` instances. `resolve` is what every gRPC
//! handler (via `global_service.rs`) calls to turn a wire `WorkspaceSelector` into the right one,
//! erroring clearly — never panicking — on an unknown selector. Each open workspace stays a wholly
//! separate `Workspace` (own store/actors/watcher/LAN/relay/file-carrier); nesting them under one
//! real `WorkspaceActor` with a shared, device-set-scoped sync `Link` is `daemon-workspace-actor`'s
//! job (todo 19), not this one — see `tasks/daemon-global-socket/notes.md`.

use crate::clock::Clock;
use crate::server::SharedWorkspace;
use crate::workspace_catalog_open::{OpenedWorkspace, WorkspaceOpenArgs};
use crate::workspace_load::LoadSlots;
use crate::workspace_registry::WorkspaceRegistry;
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex, PoisonError, RwLock};
use std::time::Duration;
use tonic::Status;
use txtodo_model::Ulid;
use txtodo_proto::v1 as pb;
use txtodo_store::WorkspaceId;

pub use crate::workspace_catalog_open::WorkspaceOpenArgs as OpenArgs;

/// Runs just before a workspace's real open, with no catalog lock held — the injection point
/// `with_open_hook` gives tests that need a slow or blockable open, in place of an environment
/// switch a production build could also honor.
pub(crate) type OpenHook = Arc<dyn Fn(&Path) + Send + Sync>;

/// The device-global catalog plus the subset of it this process has actually opened.
///
/// Opening is state-tracked, not lock-serialized (task `daemon-early-bind`): `open` only ever holds
/// finished workspaces, and `slots` tracks every registered one through Queued → Loading → Ready
/// or Failed, so a slow open never blocks a call on a workspace that is already open.
pub struct WorkspaceCatalog {
    pub(crate) registry: Mutex<WorkspaceRegistry>,
    pub(crate) open: RwLock<HashMap<WorkspaceId, OpenedWorkspace>>,
    pub(crate) open_args: WorkspaceOpenArgs,
    pub(crate) clock: Arc<dyn Clock>,
    pub(crate) slots: LoadSlots,
    /// How long a request for a workspace that is still loading waits before `Unavailable`.
    pub(crate) load_wait: Duration,
    pub(crate) open_hook: Option<OpenHook>,
    /// When each workspace's `last_active_ms` was last written, so a busy one costs one registry
    /// write per `TOUCH_EVERY_MS`, not one per request.
    pub(crate) last_touch: Mutex<HashMap<WorkspaceId, u64>>,
}

/// Default bound on a request waiting for a workspace that is still loading: the client-side spawn
/// timeout a cold daemon used to be given for the *whole* open pass, so a CLI call issued right
/// after a cold start waits about as long as it did before the socket was bound early.
pub const DEFAULT_LOAD_WAIT: Duration = Duration::from_secs(120);

impl WorkspaceCatalog {
    /// Wraps a registry and the settings every workspace this catalog opens will share.
    pub fn new(
        registry: WorkspaceRegistry,
        open_args: WorkspaceOpenArgs,
        clock: Arc<dyn Clock>,
    ) -> WorkspaceCatalog {
        WorkspaceCatalog {
            registry: Mutex::new(registry),
            open: RwLock::new(HashMap::new()),
            open_args,
            clock,
            slots: LoadSlots::default(),
            load_wait: DEFAULT_LOAD_WAIT,
            open_hook: None,
            last_touch: Mutex::new(HashMap::new()),
        }
    }

    /// Overrides how long a request waits for a loading workspace (tests use milliseconds).
    #[must_use]
    pub fn with_load_wait(mut self, wait: Duration) -> WorkspaceCatalog {
        self.load_wait = wait;
        self
    }

    /// Runs `hook(root)` at the start of every workspace open, with no lock held. For tests that
    /// need an open to be slow or to block until released.
    #[must_use]
    pub fn with_open_hook(
        mut self,
        hook: impl Fn(&Path) + Send + Sync + 'static,
    ) -> WorkspaceCatalog {
        self.open_hook = Some(Arc::new(hook));
        self
    }

    /// Registers (idempotent) and opens `dir` directly — the `--dir` bridge `main.rs` uses so
    /// today's whole single-workspace test suite keeps working unmodified.
    pub fn open_dir_bridge(&self, dir: &Path) -> Result<WorkspaceId, Status> {
        self.open_one(dir)
    }

    /// Opens every root the registry already knows about. Per-workspace failures are logged and
    /// skipped — one bad workspace must not take the whole daemon down — and a registered root
    /// that no longer exists on disk is skipped silently (nothing to open; a future task's
    /// migration/repair concern, not this one's). Returns the count actually opened.
    pub fn open_all_registered(&self) -> usize {
        let Some(entries) = self.list_registered() else {
            return 0;
        };
        entries
            .into_iter()
            .filter(|entry| entry.root_exists)
            .filter(|entry| self.open_registered_entry(&entry.root))
            .count()
    }

    pub(crate) fn list_registered(&self) -> Option<Vec<crate::workspace_registry::WorkspaceEntry>> {
        let registry = self.registry.lock().unwrap_or_else(PoisonError::into_inner);
        match registry.list() {
            Ok(entries) => Some(entries),
            Err(e) => {
                tracing::warn!(error = %e, "could not list the workspace registry");
                None
            }
        }
    }

    /// `WorkspaceAdd` RPC: registers `root` (idempotent) without opening it — never touches
    /// `root/.txtodo/` beyond the canonicalization `WorkspaceRegistry::add` already does.
    pub fn add_registered(
        &self,
        root: &Path,
    ) -> Result<crate::workspace_registry::WorkspaceEntry, Status> {
        let id = {
            let mut registry = self.registry.lock().unwrap_or_else(PoisonError::into_inner);
            registry.add(root, self.clock.as_ref()).map_err(|e| {
                Status::invalid_argument(format!("register {}: {e}", root.display()))
            })?
        };
        let registry = self.registry.lock().unwrap_or_else(PoisonError::into_inner);
        registry
            .get(id)
            .map_err(|e| Status::internal(format!("look up workspace {id}: {e}")))?
            .ok_or_else(|| Status::internal(format!("workspace {id} vanished after registering")))
    }

    /// `WorkspaceRemove` RPC: un-registers `id` and drops it from `open` if this process had it
    /// open (stopping its watcher/LAN/relay/file-carrier tasks via `OpenedWorkspace`'s `Drop`) —
    /// never touches `root/.txtodo/` on disk. `false` for an unknown id.
    pub fn remove_registered(&self, id: WorkspaceId) -> Result<bool, Status> {
        let removed = {
            let mut registry = self.registry.lock().unwrap_or_else(PoisonError::into_inner);
            registry
                .remove(id, self.clock.as_ref())
                .map_err(|e| Status::internal(format!("remove workspace {id}: {e}")))?
        };
        self.open
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&id);
        self.slots.forget(id);
        Ok(removed)
    }

    /// `WorkspaceList` RPC: every registered workspace, oldest first.
    pub fn list_registered_entries(
        &self,
    ) -> Result<Vec<crate::workspace_registry::WorkspaceEntry>, Status> {
        let registry = self.registry.lock().unwrap_or_else(PoisonError::into_inner);
        registry
            .list()
            .map_err(|e| Status::internal(format!("list workspace registry: {e}")))
    }

    /// One `open_all_registered` step: `true` on success, logged-and-`false` on failure — a bad
    /// workspace is skipped, never fatal to the whole daemon.
    fn open_registered_entry(&self, root: &Path) -> bool {
        match self.open_one(root) {
            Ok(_) => true,
            Err(status) => {
                tracing::warn!(root = %root.display(), error = %status, "workspace_open_failed");
                false
            }
        }
    }

    /// Registers (idempotent) and opens `root`; returns the (possibly already-open) id. Blocks
    /// until it is open, or until `load_wait` when another caller's open of it is still running.
    fn open_one(&self, root: &Path) -> Result<WorkspaceId, Status> {
        let id = {
            let mut registry = self.registry.lock().unwrap_or_else(PoisonError::into_inner);
            registry.add(root, self.clock.as_ref()).map_err(|e| {
                Status::invalid_argument(format!("register {}: {e}", root.display()))
            })?
        };
        self.ensure_open(id, root)?;
        Ok(id)
    }

    /// Resolves a wire selector to the workspace it names, opening it lazily (and auto-
    /// registering an unknown `path`) as needed. Never panics.
    pub fn resolve(
        &self,
        selector: Option<&pb::WorkspaceSelector>,
    ) -> Result<SharedWorkspace, Status> {
        let ws = self.resolve_inner(selector)?;
        self.note_use(&ws);
        Ok(ws)
    }

    fn resolve_inner(
        &self,
        selector: Option<&pb::WorkspaceSelector>,
    ) -> Result<SharedWorkspace, Status> {
        match selector.and_then(|s| s.selector.clone()) {
            Some(pb::workspace_selector::Selector::WorkspaceId(text)) => self.resolve_id(&text),
            Some(pb::workspace_selector::Selector::Path(path)) => {
                let id = self.open_one(Path::new(&path))?;
                self.open_ws(id)
            }
            None => self.resolve_sole_open(),
        }
    }

    /// The answer to `resolve` when it needs no open and no wait, else `None` (the caller then
    /// resolves on a blocking thread): a workspace id that is already open, or no selector at all.
    pub(crate) fn resolve_without_waiting(
        &self,
        selector: Option<&pb::WorkspaceSelector>,
    ) -> Option<Result<SharedWorkspace, Status>> {
        let answer = self.resolve_without_waiting_inner(selector)?;
        if let Ok(ws) = &answer {
            self.note_use(ws);
        }
        Some(answer)
    }

    fn resolve_without_waiting_inner(
        &self,
        selector: Option<&pb::WorkspaceSelector>,
    ) -> Option<Result<SharedWorkspace, Status>> {
        match selector.and_then(|s| s.selector.as_ref()) {
            None => Some(self.resolve_sole_open()),
            Some(pb::workspace_selector::Selector::WorkspaceId(text)) => {
                let ulid = Ulid::parse(text)?;
                self.open_ws(WorkspaceId::new(ulid)).ok().map(Ok)
            }
            // Every CLI/desktop call names its workspace by path: match it against the roots
            // already open (one `canonicalize`, no registry write) so the common case stays off the
            // blocking pool. A miss just takes the slow path, which registers and opens.
            Some(pb::workspace_selector::Selector::Path(path)) => {
                let canonical = std::fs::canonicalize(path).ok()?;
                let open = self.open.read().unwrap_or_else(PoisonError::into_inner);
                open.values()
                    .find(|o| {
                        o.ws.read().unwrap_or_else(PoisonError::into_inner).root() == canonical
                    })
                    .map(|o| Ok(Arc::clone(&o.ws)))
            }
        }
    }

    fn resolve_id(&self, text: &str) -> Result<SharedWorkspace, Status> {
        let ulid = Ulid::parse(text)
            .ok_or_else(|| Status::invalid_argument(format!("{text:?} is not a ULID")))?;
        let id = WorkspaceId::new(ulid);
        if let Ok(ws) = self.open_ws(id) {
            return Ok(ws);
        }
        let entry = {
            let registry = self.registry.lock().unwrap_or_else(PoisonError::into_inner);
            registry
                .get(id)
                .map_err(|e| Status::internal(format!("look up workspace {id}: {e}")))?
        };
        let Some(entry) = entry.filter(|e| e.root_exists) else {
            return Err(Status::not_found(format!("no workspace {id}")));
        };
        self.open_one(&entry.root)?;
        self.open_ws(id)
    }

    fn open_ws(&self, id: WorkspaceId) -> Result<SharedWorkspace, Status> {
        self.open
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&id)
            .map(|o| Arc::clone(&o.ws))
            .ok_or_else(|| Status::not_found(format!("no workspace {id}")))
    }

    /// Unset/absent selector: the single-workspace bridge every `--dir`-started daemon (and
    /// today's whole test suite, which never sets `.workspace` on any request) relies on.
    fn resolve_sole_open(&self) -> Result<SharedWorkspace, Status> {
        // While any open is still ahead, the count of open workspaces is still growing: 0 would say
        // "none open", 1 would succeed by luck, then 2 would turn "ambiguous". Say so instead.
        if self.slots.pending() > 0 {
            return Err(Status::unavailable("workspace loading"));
        }
        let open = self.open.read().unwrap_or_else(PoisonError::into_inner);
        match open.len() {
            0 => Err(Status::failed_precondition(
                "no workspace is open; start the daemon with --dir <workspace>, or register one first",
            )),
            1 => Ok(open
                .values()
                .next()
                .map(|o| Arc::clone(&o.ws))
                .unwrap_or_else(|| unreachable!("len() == 1 guarantees a first element"))),
            n => Err(Status::failed_precondition(format!(
                "ambiguous: {n} workspaces are open; the request must name one via WorkspaceSelector"
            ))),
        }
    }
}
