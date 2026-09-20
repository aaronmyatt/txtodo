//! The catalog's opening half (task `daemon-early-bind`): `ensure_open` (one open per root however
//! many callers ask), the load order, and the background loader `main.rs` starts once the socket
//! is bound. Same `impl WorkspaceCatalog`, split from `workspace_catalog.rs` for its file budget.

use crate::workspace_catalog::WorkspaceCatalog;
use crate::workspace_catalog_open::open_workspace_full;
use crate::workspace_load::{Acquired, LoadState};
use crate::workspace_registry::WorkspaceEntry;
use std::path::{Path, PathBuf};
use std::sync::{Arc, PoisonError};
use std::time::{Instant, UNIX_EPOCH};
use tonic::Status;
use txtodo_store::WorkspaceId;

/// How recently a workspace was used, in unix milliseconds: `last_active_ms` when a request ever
/// resolved it, else the newest write to its root `todo.txt` (one `stat`, nothing opened), else
/// the time it was registered.
fn recency_ms(entry: &WorkspaceEntry) -> u64 {
    entry.last_active_ms.unwrap_or_else(|| {
        std::fs::metadata(entry.root.join("todo.txt"))
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .and_then(|d| u64::try_from(d.as_millis()).ok())
            .unwrap_or(entry.added_at_ms)
    })
}

/// How often one workspace's `last_active_ms` is written back to the registry.
const TOUCH_EVERY_MS: u64 = 30_000;

/// Device-level counts for `Health`: how many workspaces are registered and where each stands.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LoadTotals {
    /// Active registered workspaces.
    pub registered: u32,
    /// Open and serving.
    pub ready: u32,
    /// Queued or loading.
    pub loading: u32,
    /// Open failed.
    pub failed: u32,
}

impl WorkspaceCatalog {
    /// Makes sure `id` (rooted at `root`) is open. Already open: returns at once. Queued or failed:
    /// opens it now, in the caller's thread — a promotion, beside whatever the loader is running.
    /// Loading: shares that open, waiting up to `load_wait`, then `Unavailable`.
    pub(crate) fn ensure_open(&self, id: WorkspaceId, root: &Path) -> Result<(), Status> {
        if self
            .open
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .contains_key(&id)
        {
            return Ok(());
        }
        match self.slots.acquire(id, self.load_wait) {
            Acquired::Ready => Ok(()),
            Acquired::TimedOut => Err(Status::unavailable("workspace loading")),
            Acquired::Open(ticket) => {
                let outcome = self.run_open(root, id);
                ticket.finish(outcome.clone().map_err(|s| s.message().to_owned()));
                outcome
            }
        }
    }

    /// The real open, with no catalog lock held: only the finished workspace is inserted, under a
    /// write lock taken for that one insert.
    fn run_open(&self, root: &Path, id: WorkspaceId) -> Result<(), Status> {
        if let Some(hook) = &self.open_hook {
            hook(root);
        }
        let started = Instant::now();
        let opened = open_workspace_full(root, id, &self.open_args, Arc::clone(&self.clock))
            .map_err(|e| Status::internal(format!("open {}: {e}", root.display())))?;
        {
            // A `WorkspaceRemove` may have landed while this open ran. `remove_registered` forgets
            // the slot first and drops from `open` second, so checking the slot under this write
            // lock is enough: either the slot is already gone (insert nothing), or this insert
            // lands before that drop and the drop takes it out again.
            let mut open = self.open.write().unwrap_or_else(PoisonError::into_inner);
            if self.slots.state(id).is_none() {
                drop(open);
                drop(opened); // stops its watcher and sync tasks (`OpenedWorkspace`'s `Drop`)
                return Err(Status::not_found(format!(
                    "workspace {id} was removed while it was opening"
                )));
            }
            open.insert(id, opened);
        }
        log_opened(id, root, started.elapsed().as_millis());
        Ok(())
    }

    /// Records that a request just resolved `ws`, at most once per `TOUCH_EVERY_MS` per workspace,
    /// so the next cold boot opens it early. Best effort: a registry failure is logged only.
    pub(crate) fn note_use(&self, ws: &crate::server::SharedWorkspace) {
        let id = ws
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .workspace_id();
        let now = self.clock.now_ms();
        {
            let mut last = self
                .last_touch
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            if last
                .get(&id)
                .is_some_and(|at| now.saturating_sub(*at) < TOUCH_EVERY_MS)
            {
                return;
            }
            last.insert(id, now);
        }
        let mut registry = self.registry.lock().unwrap_or_else(PoisonError::into_inner);
        if let Err(e) = registry.touch(id, self.clock.as_ref()) {
            log_touch_failed(id, &e);
        }
    }

    /// `WorkspaceList`/`Health` totals, from the registry and the load slots; nothing opened.
    pub fn load_totals(&self) -> LoadTotals {
        let mut totals = LoadTotals {
            registered: self
                .list_registered()
                .map_or(0, |e| u32::try_from(e.len()).unwrap_or(u32::MAX)),
            ..LoadTotals::default()
        };
        for (_, state) in self.slots.snapshot() {
            match state {
                LoadState::Ready => totals.ready += 1,
                LoadState::Queued | LoadState::Loading => totals.loading += 1,
                LoadState::Failed(_) => totals.failed += 1,
            }
        }
        totals
    }

    /// `id`'s load state, or `None` for a workspace this process never queued or opened.
    pub fn load_state(&self, id: WorkspaceId) -> Option<LoadState> {
        self.slots.state(id)
    }

    /// How many workspaces are still `Queued` or `Loading`.
    pub fn load_pending(&self) -> usize {
        self.slots.pending()
    }

    /// Queues every registered root that still exists, most recently used first, and returns them
    /// in that order for [`Self::spawn_loader`]. Nothing is opened here.
    pub fn queue_registered(&self) -> Vec<(WorkspaceId, PathBuf)> {
        let Some(entries) = self.list_registered() else {
            return Vec::new();
        };
        let mut live: Vec<WorkspaceEntry> = entries.into_iter().filter(|e| e.root_exists).collect();
        live.sort_by_key(|e| std::cmp::Reverse(recency_ms(e)));
        for entry in &live {
            self.slots.queue(entry.id);
        }
        live.into_iter().map(|e| (e.id, e.root)).collect()
    }

    /// Starts the background loader: one thread, opening `order` one at a time. A workspace a
    /// request already promoted (or that is loading or done) is skipped; a failure is logged and
    /// never ends the pass. The opens are synchronous and heavy, hence a thread of its own.
    pub fn spawn_loader(
        self: &Arc<Self>,
        order: Vec<(WorkspaceId, PathBuf)>,
    ) -> std::io::Result<std::thread::JoinHandle<()>> {
        let catalog = Arc::clone(self);
        // An open spawns tokio tasks (the watcher's drain loop, LAN, relay): this thread is not a
        // runtime thread, so it enters the caller's runtime for its whole life.
        let runtime = tokio::runtime::Handle::current();
        std::thread::Builder::new()
            .name("txtodod-loader".to_owned())
            .spawn(move || {
                let _enter = runtime.enter();
                let started = Instant::now();
                let total = order.len();
                for (id, root) in order {
                    catalog.load_queued(id, &root);
                }
                log_loaded(total, started.elapsed().as_millis());
            })
    }

    fn load_queued(&self, id: WorkspaceId, root: &Path) {
        let Some(ticket) = self.slots.try_start(id) else {
            return;
        };
        let outcome = self.run_open(root, id);
        if let Err(status) = &outcome {
            log_open_failed(root, status);
        }
        ticket.finish(outcome.map_err(|s| s.message().to_owned()));
    }
}

fn log_touch_failed(
    id: WorkspaceId,
    error: &crate::workspace_registry_error::WorkspaceRegistryError,
) {
    tracing::warn!(workspace = %id, error = %error, "workspace_touch_failed");
}

fn log_opened(id: WorkspaceId, root: &Path, ms: u128) {
    tracing::info!(workspace = %id, root = %root.display(), ms = %ms, "workspace_opened");
}

fn log_open_failed(root: &Path, status: &Status) {
    tracing::warn!(root = %root.display(), error = %status, "workspace_open_failed");
}

fn log_loaded(workspaces: usize, ms: u128) {
    tracing::info!(workspaces, ms = %ms, "workspaces_loaded");
}
