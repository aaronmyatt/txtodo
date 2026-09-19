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

/// How recently a workspace was used, in unix milliseconds: the newest write to its root
/// `todo.txt` (one `stat`, nothing opened), else the time it was registered.
fn recency_ms(entry: &WorkspaceEntry) -> u64 {
    std::fs::metadata(entry.root.join("todo.txt"))
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .and_then(|d| u64::try_from(d.as_millis()).ok())
        .unwrap_or(entry.added_at_ms)
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
        self.open
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(id, opened);
        log_opened(id, root, started.elapsed().as_millis());
        Ok(())
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

fn log_opened(id: WorkspaceId, root: &Path, ms: u128) {
    tracing::info!(workspace = %id, root = %root.display(), ms = %ms, "workspace_opened");
}

fn log_open_failed(root: &Path, status: &Status) {
    tracing::warn!(root = %root.display(), error = %status, "workspace_open_failed");
}

fn log_loaded(workspaces: usize, ms: u128) {
    tracing::info!(workspaces, ms = %ms, "workspaces_loaded");
}
