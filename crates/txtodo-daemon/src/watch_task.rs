//! The async side of the watcher: drains raw `notify` events, applies ignore rules and the
//! debounce, and routes to actors (`ExternalChange`) or the workspace (`discover`). One task per
//! daemon. The `notify` watcher handle must stay alive for as long as the task runs.

use crate::clock::Clock;
use crate::debounce::Debouncer;
use crate::server::SharedWorkspace;
use crate::watcher::{RawEvent, Routed, ingest, start as start_notify};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

/// When idle with nothing pending, how often the drain wakes to check for shutdown.
pub const IDLE_TICK_MS: u64 = 1_000;

fn read(ws: &SharedWorkspace) -> std::sync::RwLockReadGuard<'_, crate::workspace::Workspace> {
    ws.read().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Starts notify on the workspace root and the drain task.
pub fn start(
    ws: SharedWorkspace,
    clock: Arc<dyn Clock>,
) -> notify::Result<(notify::RecommendedWatcher, JoinHandle<()>)> {
    let root = read(&ws).root().to_path_buf();
    let (watcher, rx) = start_notify(&root)?;
    read(&ws).stats().saw_event(clock.now_ms());
    let task = tokio::spawn(drain(ws, clock, rx));
    Ok((watcher, task))
}

async fn drain(ws: SharedWorkspace, clock: Arc<dyn Clock>, mut rx: mpsc::Receiver<RawEvent>) {
    let mut deb = Debouncer::default();
    // Ends when the notify side drops its sender (the watcher handle was dropped).
    loop {
        let wait = deb
            .next_deadline()
            .map_or(Duration::from_millis(IDLE_TICK_MS), |d| {
                d.saturating_duration_since(clock.now_instant())
            });
        let routed = tokio::select! {
            ev = rx.recv() => match ev {
                Some(ev) => {
                    read(&ws).stats().saw_event(clock.now_ms());
                    ingest(&mut deb, ev, clock.now_instant())
                }
                None => return,
            },
            () = tokio::time::sleep(wait) => None,
        };
        if let Some(Routed::Directory(dir)) = routed {
            discover(&ws, &dir);
        }
        for path in deb.drain_due(clock.now_instant()) {
            route_document(&ws, &path).await;
        }
    }
}

fn discover(ws: &SharedWorkspace, dir: &Path) {
    let mut guard = ws
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    // A walk error here is a directory vanishing between event and walk; the next event retries.
    if let Err(e) = guard.discover(dir) {
        tracing::warn!(dir = %dir.display(), error = %e, "discover failed");
    }
}

/// A debounced document path: its actor gets `ExternalChange`; an unknown document in a known
/// directory is registered, which adopts the file.
async fn route_document(ws: &SharedWorkspace, path: &Path) {
    let handle = read(ws).actor_for_disk(path).cloned();
    match handle {
        Some(h) => {
            // The actor logs its own failure; the watcher has nobody to report to.
            let _delivered = h.external_change().await;
        }
        None => {
            if let Some(parent) = path.parent() {
                discover(ws, parent);
            }
        }
    }
}
