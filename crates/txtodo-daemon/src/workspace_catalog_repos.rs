//! Repos in the catalog (task `two-kinds-of-workspace`, ADR 0043): the scan that finds git clones
//! holding a todo list and registers them as workspaces, the roots it walks, and the schedule.
//!
//! A scan only **registers**: nothing is opened, so a scan of a home folder with thirty clones costs
//! a registry write each and no watcher. The loader opens them at the next start, after the
//! workspaces the person used, and a first use opens one at once. A folder taken off the registry
//! by hand stays off (`root_was_removed`); a nested clone the registry refuses for overlapping its
//! parent is logged and skipped. Registering also writes `.txtodo/` into the clone's own
//! `info/exclude` (`WorkspaceRegistry::add`). Every repo the scan finds is also readied for git: its
//! lists set to merge line by line and the merge driver in its config (`repo_git.rs`), the new ones
//! at registration and the registered ones again on each scan, so the person runs nothing.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use tonic::Status;

use crate::repo_scan;
use crate::workspace_catalog::WorkspaceCatalog;

/// How long after the daemon starts the first scan waits, so the socket is bound and the loader has
/// begun opening the workspaces in use before a walk of the home folder competes with it.
pub const FIRST_SCAN_AFTER: Duration = Duration::from_secs(5);

/// Between scheduled scans.
pub const SCAN_EVERY: Duration = Duration::from_secs(6 * 60 * 60);

/// What the last finished scan found.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ScanReport {
    /// Repos it found.
    pub found: u32,
    /// Of those, how many it registered that were new.
    pub registered: u32,
}

/// The scan's running flag and last report, one per catalog.
#[derive(Default)]
pub(crate) struct ScanState {
    running: AtomicBool,
    last: Mutex<ScanReport>,
}

/// Held while a scan runs; dropping it clears the flag, whichever way the scan ended.
pub(crate) struct ScanGuard(Arc<WorkspaceCatalog>);

impl Drop for ScanGuard {
    fn drop(&mut self) {
        self.0.scan.running.store(false, Ordering::SeqCst);
    }
}

impl WorkspaceCatalog {
    /// Gives a registry with no scan roots its defaults, once (`Registry::seed_scan_roots`).
    pub fn seed_scan_roots(&self, defaults: &[PathBuf]) {
        let defaults: Vec<String> = defaults.iter().map(|p| p.display().to_string()).collect();
        let result = self
            .registry
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .seed_scan_roots(&defaults, self.clock.as_ref());
        if let Err(e) = result {
            tracing::warn!(error = %e, "repo_scan_seed_failed");
        }
    }

    /// The folders scanned, as stored.
    pub(crate) fn scan_roots(&self) -> Result<Vec<String>, Status> {
        let roots = self
            .registry
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .scan_roots();
        roots.map_err(|e| Status::internal(format!("read scan roots: {e}")))
    }

    /// Adds `add` and removes `remove` from the scan roots. A root that is not an absolute path is
    /// refused (`INVALID_ARGUMENT`) before anything changes.
    pub(crate) fn edit_scan_roots(&self, add: &[String], remove: &[String]) -> Result<(), Status> {
        if let Some(bad) = add.iter().find(|r| !Path::new(r).is_absolute()) {
            return Err(Status::invalid_argument(format!(
                "scan root {bad:?} is not an absolute path"
            )));
        }
        let mut registry = self.registry.lock().unwrap_or_else(PoisonError::into_inner);
        let failed = |e: crate::workspace_registry_error::WorkspaceRegistryError| {
            Status::invalid_argument(format!("scan roots: {e}"))
        };
        for root in add {
            registry
                .add_scan_root(root, self.clock.as_ref())
                .map_err(failed)?;
        }
        for root in remove {
            registry.remove_scan_root(root).map_err(failed)?;
        }
        Ok(())
    }

    /// When the last scan finished, in unix milliseconds; 0 for never.
    pub(crate) fn last_scan_ms(&self) -> u64 {
        let at = self
            .registry
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .last_scan_ms();
        at.ok().flatten().unwrap_or(0)
    }

    /// Whether a scan is running now.
    pub(crate) fn scan_running(&self) -> bool {
        self.scan.running.load(Ordering::SeqCst)
    }

    /// What the last finished scan found.
    pub(crate) fn scan_report(&self) -> ScanReport {
        *self
            .scan
            .last
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    /// Claims the right to scan: `None` when a scan is running already.
    pub(crate) fn try_begin_scan(self: &Arc<Self>) -> Option<ScanGuard> {
        let was_running = self.scan.running.swap(true, Ordering::SeqCst);
        (!was_running).then(|| ScanGuard(Arc::clone(self)))
    }

    /// Scans the roots and registers the repos it finds that are new. Blocks: it walks the disk.
    pub(crate) fn run_scan(&self, _guard: ScanGuard) -> Result<ScanReport, Status> {
        // Canonical, like the skips and the registry's roots (macOS `/var` is `/private/var`), so a
        // folder the scan must not enter is recognised whichever way the root was written.
        // Ref: https://doc.rust-lang.org/std/fs/fn.canonicalize.html
        let canon = |p: PathBuf| p.canonicalize().unwrap_or(p);
        let roots: Vec<PathBuf> = self
            .scan_roots()?
            .into_iter()
            .map(|r| canon(r.into()))
            .collect();
        let scanned = repo_scan::scan(&roots, &self.scan_skips());
        let active: HashSet<PathBuf> = self
            .list_registered_entries()?
            .into_iter()
            .map(|e| e.root)
            .collect();
        let mut report = ScanReport {
            found: u32::try_from(scanned.repos.len()).unwrap_or(u32::MAX),
            registered: 0,
        };
        for repo in &scanned.repos {
            let root = repo.canonicalize().unwrap_or_else(|_| repo.clone());
            if active.contains(&root) {
                // Registered before: ready it again, in case it was before this was automatic or
                // someone undid it. Only `add` does this for a repo that is new.
                ready_clone(&root);
            } else if self.register_found(&root) {
                report.registered += 1;
            }
        }
        // Best effort: a registry that cannot record the time still has its repos.
        let _ = self
            .registry
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .record_scan(self.clock.as_ref());
        *self
            .scan
            .last
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = report;
        log_scanned(&scanned, report);
        Ok(report)
    }

    /// The daemon's own folders, which a scan never enters: the data dir the lists and mirrors sit in.
    fn scan_skips(&self) -> Vec<PathBuf> {
        let of = |root: Option<&PathBuf>| root.and_then(|r| r.parent()).map(Path::to_path_buf);
        [of(self.lists_root.get()), of(self.remote_root.get())]
            .into_iter()
            .flatten()
            .collect()
    }

    /// Registers one repo the scan found: `true` when it is new. A folder the person removed stays
    /// off, and one the registry refuses (it overlaps another workspace) is logged and skipped.
    fn register_found(&self, root: &Path) -> bool {
        let mut registry = self.registry.lock().unwrap_or_else(PoisonError::into_inner);
        if registry.root_was_removed(root).unwrap_or(false) {
            return false;
        }
        match registry.add(root, self.clock.as_ref()) {
            Ok(_) => true,
            Err(e) => {
                tracing::debug!(root = %root.display(), error = %e, "repo_scan_skipped");
                false
            }
        }
    }

    /// Scans once a little after start and then every [`SCAN_EVERY`], until the runtime stops. A
    /// registry with no scan roots makes each pass a no-op.
    pub fn start_repo_scan(self: &Arc<Self>) {
        let catalog = Arc::clone(self);
        tokio::spawn(async move {
            tokio::time::sleep(FIRST_SCAN_AFTER).await;
            loop {
                let pass = Arc::clone(&catalog);
                // Ref: https://docs.rs/tokio/latest/tokio/task/fn.spawn_blocking.html
                let done = tokio::task::spawn_blocking(move || {
                    pass.try_begin_scan().map(|guard| pass.run_scan(guard))
                })
                .await;
                if let Err(e) = done {
                    tracing::warn!(error = %e, "repo_scan_panicked");
                }
                tokio::time::sleep(SCAN_EVERY).await;
            }
        });
    }
}

/// `repo_git::prepare` for a registered repo; a refusal is logged and the scan goes on.
fn ready_clone(root: &Path) {
    match crate::repo_git::prepare(root) {
        Ok(done) => log_prepared(root, done),
        Err(e) => log_prepare_failed(root, &e),
    }
}

fn log_prepared(root: &Path, done: crate::repo_git::Prepared) {
    if done.any() {
        tracing::info!(root = %root.display(), ?done, "repo_git_prepared");
    }
}

fn log_prepare_failed(root: &Path, e: &std::io::Error) {
    tracing::warn!(root = %root.display(), error = %e, "repo_git_prepare_failed");
}

fn log_scanned(scanned: &repo_scan::Scanned, report: ScanReport) {
    tracing::info!(
        found = report.found,
        registered = report.registered,
        dirs = scanned.dirs,
        truncated = scanned.truncated,
        "repo_scan_done"
    );
}
