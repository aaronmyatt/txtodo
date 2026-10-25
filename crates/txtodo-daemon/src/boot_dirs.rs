//! The folders the catalog keeps the workspaces it makes itself in (`main.rs`): mirrors of a peer's
//! offers (task remote-workspace-mirror) and named lists (task two-kinds-of-workspace). Both sit
//! beside the default workspace, or under the bridge's own state dir with `--dir`.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use txtodo_daemon::workspace_catalog::WorkspaceCatalog;
use txtodo_daemon::workspace_registry_paths::{self, RegistryEnv};

use crate::Args;

/// `remote/` and `lists/`: beside the default workspace, or under the bridge's own state dir with
/// `--dir`.
fn folders(args: &Args, env: &RegistryEnv, state_dir: &Path) -> (PathBuf, PathBuf) {
    match args.dir {
        Some(_) => (state_dir.join("remote"), state_dir.join("lists")),
        None => (
            workspace_registry_paths::remote_workspaces_dir_for(env),
            workspace_registry_paths::lists_dir_for(env),
        ),
    }
}

/// Starts the offer mirror in `remote/`, sets `lists/` as the lists root and, in global mode, the
/// repo scan. A folder that cannot be made is logged and never fatal: the daemon runs without that
/// feature, nothing else is lost.
pub(crate) fn start(
    catalog: &Arc<WorkspaceCatalog>,
    args: &Args,
    env: &RegistryEnv,
    state_dir: &Path,
) {
    let (remote, lists) = folders(args, env, state_dir);
    catalog.start_offer_mirror(&remote);
    if let Err(e) = catalog.set_lists_root(&lists) {
        tracing::warn!(dir = %lists.display(), error = %e, "lists_root_unavailable");
    }
    if args.dir.is_none() {
        start_repo_scan(catalog);
    }
}

/// Seeds the home folder as the one default scan root and starts the scan schedule (task
/// two-kinds-of-workspace). Seeding is left out of a daemon whose socket or registry an environment
/// variable moved: that is a test or an isolated run, and it must never walk the real home folder.
/// The schedule still starts; a registry with no roots makes each pass a no-op.
fn start_repo_scan(catalog: &Arc<WorkspaceCatalog>) {
    let isolated = ["TXTODO_SOCKET", "TXTODO_REGISTRY_DB"]
        .iter()
        .any(|var| std::env::var_os(var).is_some());
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"));
    if let (false, Some(home)) = (isolated, home) {
        catalog.seed_scan_roots(&[home.into()]);
    }
    catalog.start_repo_scan();
}
