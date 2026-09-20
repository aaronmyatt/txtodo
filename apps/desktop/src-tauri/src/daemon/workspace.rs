//! `DaemonClient` workspace-registry RPCs (ADR 0025, task `desktop-workspace-switcher`), split
//! out of `daemon.rs` the way `spawn.rs` already is. Unlike every other RPC these three never
//! carry `self.selector` — they target the registry itself, not an already-open workspace,
//! mirroring `crates/txtodo-cli/src/client_workspace.rs`'s own three methods exactly.

use super::{DaemonClient, DaemonError};
use txtodo_proto::v1 as pb;

impl DaemonClient {
    /// Registers `root` (idempotent: an already-active root returns its existing entry) without
    /// opening it.
    pub async fn workspace_add(
        &mut self,
        root: &std::path::Path,
    ) -> Result<pb::WorkspaceInfo, DaemonError> {
        let req = pb::WorkspaceAddRequest {
            root: root.display().to_string(),
        };
        Ok(self.inner.workspace_add(req).await?.into_inner())
    }

    /// Un-registers a workspace id; never touches `root/.txtodo/` on disk.
    pub async fn workspace_remove(&mut self, id: &str) -> Result<bool, DaemonError> {
        let req = pb::WorkspaceRemoveRequest {
            workspace_id: id.to_owned(),
        };
        Ok(self.inner.workspace_remove(req).await?.into_inner().removed)
    }

    /// Every registered workspace, oldest first.
    pub async fn workspace_list(&mut self) -> Result<Vec<pb::WorkspaceInfo>, DaemonError> {
        Ok(self
            .inner
            .workspace_list(pb::WorkspaceListRequest {})
            .await?
            .into_inner()
            .workspaces)
    }

    /// The daemon's own version and release date: a selector-less `Health`, which the daemon
    /// answers at once with the device totals even while workspaces are still opening, so this
    /// never waits on an open (task version-info). An older daemon sends an empty date.
    pub async fn daemon_build(&mut self) -> Result<(String, String), DaemonError> {
        let health = self
            .inner
            .health(pb::HealthRequest { workspace: None })
            .await?
            .into_inner();
        Ok((health.version, health.release_date))
    }
}
