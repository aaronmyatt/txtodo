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
