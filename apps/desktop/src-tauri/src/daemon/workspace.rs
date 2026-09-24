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

    /// The current workspace's layout (task workspace-layout): where the root list's `ref:`
    /// directories live. Carries this connection's selector, so it is the picked workspace's.
    pub async fn workspace_layout(&mut self) -> Result<pb::WorkspaceLayoutInfo, DaemonError> {
        let req = pb::WorkspaceLayoutRequest {
            workspace: self.selector.clone(),
            ..pb::WorkspaceLayoutRequest::default()
        };
        Ok(self.inner.workspace_layout(req).await?.into_inner())
    }

    /// The root list of `selector`'s workspace (its layout's `todo_file`), without touching this
    /// client's own selector: the universal view reads every workspace's list. An older daemon
    /// that does not know the RPC has only ever had `todo.txt`.
    pub async fn root_list_for(&mut self, selector: pb::WorkspaceSelector) -> String {
        let req = pb::WorkspaceLayoutRequest {
            workspace: Some(selector),
            ..pb::WorkspaceLayoutRequest::default()
        };
        match self.inner.workspace_layout(req).await {
            Ok(resp) => resp.into_inner().todo_file,
            Err(_) => "todo.txt".to_owned(),
        }
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

    /// Why offers from paired devices are blocked, and how long ago that was seen; empty and 0
    /// when they are not (task control-channel-keystore-visibility). Selector-less `Health`, like
    /// [`Self::daemon_build`]: the problem is device-level.
    pub async fn offers_problem(&mut self) -> Result<(String, u64), DaemonError> {
        let health = self
            .inner
            .health(pb::HealthRequest { workspace: None })
            .await?
            .into_inner();
        Ok((health.offers_problem, health.offers_problem_age_ms))
    }
}

impl DaemonClient {
    /// A line's `ref:` directory: resolved read-only, or claimed (tag + directory, one op batch,
    /// daemon-side) with `ensure` — the detail view's "start a sub-list" step (task
    /// desktop-sublist-start). The client never computes a slug or makes a directory itself.
    pub async fn ref_dir(
        &mut self,
        path: &str,
        task: pb::TaskRef,
        ensure: bool,
    ) -> Result<pb::RefDirInfo, DaemonError> {
        let req = pb::RefDirRequest {
            path: path.to_owned(),
            task: Some(task),
            ensure,
            workspace: self.selector.clone(),
        };
        Ok(self.inner.ref_dir(req).await?.into_inner())
    }
}
