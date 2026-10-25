//! The folders the catalog keeps the workspaces it makes itself in (`main.rs`): mirrors of a peer's
//! offers (task remote-workspace-mirror) and named lists (task two-kinds-of-workspace). Both sit
//! beside the default workspace, or under the bridge's own state dir with `--dir`.

use std::path::Path;
use std::sync::Arc;

use txtodo_daemon::workspace_catalog::WorkspaceCatalog;
use txtodo_daemon::workspace_registry_paths::{self, RegistryEnv};

use crate::Args;

/// Starts the offer mirror in `remote/` and sets `lists/` as the lists root. A folder that cannot
/// be made is logged and never fatal: the daemon runs without that feature, nothing else is lost.
pub(crate) fn start(
    catalog: &Arc<WorkspaceCatalog>,
    args: &Args,
    env: &RegistryEnv,
    state_dir: &Path,
) {
    let (remote, lists) = match args.dir {
        Some(_) => (state_dir.join("remote"), state_dir.join("lists")),
        None => (
            workspace_registry_paths::remote_workspaces_dir_for(env),
            workspace_registry_paths::lists_dir_for(env),
        ),
    };
    catalog.start_offer_mirror(&remote);
    if let Err(e) = catalog.set_lists_root(&lists) {
        tracing::warn!(dir = %lists.display(), error = %e, "lists_root_unavailable");
    }
}
