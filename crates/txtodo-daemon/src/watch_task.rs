//! The async side of the watcher: drains raw `notify` events, applies ignore rules and the
//! debounce, and routes to actors (`ExternalChange`) or the workspace (`discover`). One task per
//! daemon. The `notify` watcher handle must stay alive for as long as the task runs.

use crate::clock::Clock;
use crate::debounce::Debouncer;
use crate::server::SharedWorkspace;
use crate::walker::{is_in_skipped_dir, walk_with};
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
    let task = tokio::spawn(drain(ws, clock, rx, root));
    Ok((watcher, task))
}

/// One long-running loop per open workspace: no `#[instrument]` span here (it would hold across
/// every `.await` below for as long as the workspace stays open, risking an unrelated task polled
/// on the same runtime thread while this one is suspended picking it up as its parent span — the
/// same hazard `main.rs`'s `daemon.boot` span design already documents and avoids). Two plain
/// `info!` events (their own tiny function, `log_watch_drain`, to keep this wrapper's own
/// complexity down) bracket `drain_loop`'s lifetime instead.
async fn drain(
    ws: SharedWorkspace,
    clock: Arc<dyn Clock>,
    mut rx: mpsc::Receiver<RawEvent>,
    root: std::path::PathBuf,
) {
    log_watch_drain_started(&root);
    drain_loop(ws, clock, &mut rx).await;
    log_watch_drain_stopped(&root);
}

fn log_watch_drain_started(root: &Path) {
    tracing::info!(root = %root.display(), "watch_drain_started");
}

fn log_watch_drain_stopped(root: &Path) {
    tracing::info!(root = %root.display(), "watch_drain_stopped");
}

async fn drain_loop(ws: SharedWorkspace, clock: Arc<dyn Clock>, rx: &mut mpsc::Receiver<RawEvent>) {
    let mut deb = Debouncer::default();
    let root = read(&ws).root().to_path_buf();
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
                    let extra = read(&ws).extra_document();
                    ingest(&mut deb, ev, clock.now_instant(), extra.as_deref())
                }
                None => return,
            },
            () = tokio::time::sleep(wait) => None,
        };
        // `notify` reports every directory a `cargo build` creates under `target/`; walking each
        // one is what starved every RPC on 2026-09-19, so skipped trees never reach `discover`.
        if let Some(Routed::Directory(dir)) = routed.filter(|r| !in_skipped_dir(&root, r)) {
            discover(&ws, &dir);
        }
        for path in deb.drain_due(clock.now_instant()) {
            if !is_in_skipped_dir(&root, &path) {
                route_document(&ws, &path).await;
            }
        }
    }
}

/// True for a routed directory that sits below a skipped tree (`walker::is_skipped_dir`).
fn in_skipped_dir(root: &Path, routed: &Routed) -> bool {
    match routed {
        Routed::Directory(dir) | Routed::Document(dir) => is_in_skipped_dir(root, dir),
    }
}

fn discover(ws: &SharedWorkspace, dir: &Path) {
    // The walk is plain filesystem I/O: run it before taking the write lock, which only the
    // registration of anything new needs. A walk error here is a directory vanishing between
    // event and walk; the next event retries.
    let extra = read(ws).extra_document();
    let Ok(found) = walk_with(dir, extra.as_deref()).inspect_err(|e| log_discover_failed(dir, e))
    else {
        return;
    };
    let mut guard = ws
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _ = guard
        .register_discovered(dir, found)
        .inspect_err(|e| log_discover_failed(dir, e));
}

fn log_discover_failed(dir: &Path, error: &dyn std::fmt::Display) {
    tracing::warn!(dir = %dir.display(), %error, "discover failed");
}

/// A debounced document path: its actor gets `ExternalChange`; an unknown document in a known
/// directory is registered, which adopts the file.
async fn route_document(ws: &SharedWorkspace, path: &Path) {
    // `txtodo.toml` is not a document: it changes where ref dirs live (task workspace-layout).
    let root = read(ws).root().to_path_buf();
    if crate::layout_reload::is_layout_file(&root, path) {
        crate::layout_reload::reload_layout(ws).await;
        return;
    }
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
