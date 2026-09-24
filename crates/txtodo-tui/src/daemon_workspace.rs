//! Device-level workspace RPCs (task `tui-revamp/tui-foundation`, split out of `daemon.rs`):
//! the registry and the offers a paired peer made. No selector, like the CLI's own
//! `client_workspace.rs`.

use txtodo_proto::v1 as pb;

use crate::daemon::{Daemon, DaemonError};

/// Workspace offer RPCs (task `workspace-offer-cli`), registry-level: no selector, like the CLI's
/// own `client_workspace.rs`.
impl Daemon {
    /// Every workspace a paired peer offered that this device has not accepted or declined.
    /// With `offers_problem` set when the daemon's offer exchange is failing (task
    /// control-channel-keystore-visibility).
    pub async fn workspace_pending_offers(
        &mut self,
    ) -> Result<pb::WorkspacePendingOffersResponse, DaemonError> {
        let req = pb::WorkspacePendingOffersRequest {};
        Ok(self.inner.workspace_pending_offers(req).await?.into_inner())
    }

    /// Mirrors an offer now, in the daemon's own folder; the daemon consumes the offer either way.
    pub async fn workspace_accept_offer(
        &mut self,
        req: pb::WorkspaceAcceptOfferRequest,
    ) -> Result<pb::WorkspaceInfo, DaemonError> {
        Ok(self.inner.workspace_accept_offer(req).await?.into_inner())
    }

    /// Discards an offer; `false` when there was none.
    pub async fn workspace_decline_offer(
        &mut self,
        req: pb::WorkspaceDeclineOfferRequest,
    ) -> Result<bool, DaemonError> {
        Ok(self
            .inner
            .workspace_decline_offer(req)
            .await?
            .into_inner()
            .declined)
    }
}

/// The registry itself, and the Universal screen's rows (task `tui-revamp/tui-foundation`).
impl Daemon {
    /// Every registered workspace, with its load state and layout.
    pub async fn workspace_list(&mut self) -> Result<pb::WorkspaceListResponse, DaemonError> {
        let req = pb::WorkspaceListRequest {};
        Ok(self.inner.workspace_list(req).await?.into_inner())
    }

    /// Registers `root` (the daemon canonicalizes it) and opens it.
    pub async fn workspace_add(&mut self, root: &str) -> Result<pb::WorkspaceInfo, DaemonError> {
        let req = pb::WorkspaceAddRequest {
            root: root.to_owned(),
        };
        Ok(self.inner.workspace_add(req).await?.into_inner())
    }

    /// Unregisters one workspace (never deletes its files); `false` when the id was unknown.
    pub async fn workspace_remove(&mut self, workspace_id: &str) -> Result<bool, DaemonError> {
        let req = pb::WorkspaceRemoveRequest {
            workspace_id: workspace_id.to_owned(),
        };
        Ok(self.inner.workspace_remove(req).await?.into_inner().removed)
    }

    /// Every root-list task across every ready workspace (`UniversalTasks`).
    pub async fn universal_tasks(
        &mut self,
        include_done: bool,
    ) -> Result<pb::UniversalTasksResponse, DaemonError> {
        let req = pb::UniversalTasksRequest { include_done };
        Ok(self.inner.universal_tasks(req).await?.into_inner())
    }
}

/// A selector naming a workspace by its id, for a call about a workspace other than the one this
/// client was built for (the Universal screen opening a row, the Activity card's streams).
pub fn workspace_id_selector(workspace_id: &str) -> pb::WorkspaceSelector {
    pb::WorkspaceSelector {
        selector: Some(pb::workspace_selector::Selector::WorkspaceId(
            workspace_id.to_owned(),
        )),
    }
}
