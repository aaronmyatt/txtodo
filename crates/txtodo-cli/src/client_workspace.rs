//! `Daemon` workspace-registry RPCs (ADR 0025, task `cli-workspace-commands`), split out of
//! `client.rs` purely for its own file-length budget. Unlike every other RPC these three never
//! carry `self.selector` — they target the registry itself, not an already-open workspace.

use crate::client::{ClientError, Daemon};
use txtodo_proto::v1::{self as pb};

impl Daemon {
    /// Registers `root` (idempotent: an already-active root returns its existing entry).
    pub fn workspace_add(&mut self, root: &str) -> Result<pb::WorkspaceInfo, ClientError> {
        let req = pb::WorkspaceAddRequest {
            root: root.to_owned(),
        };
        let rep = self
            .rt
            .block_on(self.client.workspace_add(req))
            .map_err(ClientError::Rpc)?;
        Ok(rep.into_inner())
    }

    /// Un-registers a workspace id; never touches `root/.txtodo/` on disk.
    pub fn workspace_remove(&mut self, id: &str) -> Result<bool, ClientError> {
        let req = pb::WorkspaceRemoveRequest {
            workspace_id: id.to_owned(),
        };
        let rep = self
            .rt
            .block_on(self.client.workspace_remove(req))
            .map_err(ClientError::Rpc)?;
        Ok(rep.into_inner().removed)
    }

    /// Every registered workspace, oldest first.
    pub fn workspace_list(&mut self) -> Result<Vec<pb::WorkspaceInfo>, ClientError> {
        let rep = self
            .rt
            .block_on(self.client.workspace_list(pb::WorkspaceListRequest {}))
            .map_err(ClientError::Rpc)?;
        Ok(rep.into_inner().workspaces)
    }

    /// The workspace's layout (task workspace-layout): reads it, or changes it with `set`.
    pub fn workspace_layout(
        &mut self,
        req: pb::WorkspaceLayoutRequest,
    ) -> Result<pb::WorkspaceLayoutInfo, ClientError> {
        let req = pb::WorkspaceLayoutRequest {
            workspace: self.selector.clone(),
            ..req
        };
        let rep = self
            .rt
            .block_on(self.client.workspace_layout(req))
            .map_err(ClientError::Rpc)?;
        Ok(rep.into_inner())
    }

    /// Health for a specific registered workspace by id, regardless of this connection's own
    /// resolved `self.selector` — `txtodo doctor`'s per-entry probe of every *other* registered
    /// workspace. Temporarily swaps the selector rather than opening a second connection.
    pub fn health_for_id(&mut self, id: &str) -> Result<pb::HealthResponse, ClientError> {
        let saved = self.selector.take();
        self.selector = Some(pb::WorkspaceSelector {
            selector: Some(pb::workspace_selector::Selector::WorkspaceId(id.to_owned())),
        });
        let result = self.health();
        self.selector = saved;
        result
    }
}

/// Workspace offer RPCs (task `workspace-offer-cli`): the pending offers a paired peer sent this
/// device, and accepting or declining one. Registry-level like the three above — no selector.
impl Daemon {
    /// Every workspace a peer offered that this device has not yet accepted or declined, plus why
    /// offers are blocked when they are (`offers_problem`, task control-channel-keystore-visibility).
    pub fn workspace_pending_offers(
        &mut self,
    ) -> Result<pb::WorkspacePendingOffersResponse, ClientError> {
        let rep = self
            .rt
            .block_on(
                self.client
                    .workspace_pending_offers(pb::WorkspacePendingOffersRequest {}),
            )
            .map_err(ClientError::Rpc)?;
        Ok(rep.into_inner())
    }

    /// Mirrors a pending offer now, in the daemon's own mirror folder (task
    /// remote-workspace-mirror). The daemon consumes the offer whether or not that succeeds.
    pub fn workspace_accept_offer(
        &mut self,
        offering_device: &str,
        workspace_id: &str,
    ) -> Result<pb::WorkspaceInfo, ClientError> {
        let req = pb::WorkspaceAcceptOfferRequest {
            offering_device: offering_device.to_owned(),
            workspace_id: workspace_id.to_owned(),
        };
        let rep = self
            .rt
            .block_on(self.client.workspace_accept_offer(req))
            .map_err(ClientError::Rpc)?;
        Ok(rep.into_inner())
    }

    /// Drops this device's copy of a workspace and takes a paired device's (task sync-drift line
    /// 8); `dry_run` only says what would move and who offers it.
    pub fn workspace_rejoin(
        &mut self,
        workspace_id: &str,
        dry_run: bool,
    ) -> Result<pb::WorkspaceRejoinResponse, ClientError> {
        let req = pb::WorkspaceRejoinRequest {
            workspace_id: workspace_id.to_owned(),
            dry_run,
        };
        let rep = self
            .rt
            .block_on(self.client.workspace_rejoin(req))
            .map_err(ClientError::Rpc)?;
        Ok(rep.into_inner())
    }

    /// Discards a pending offer; `false` when there was no such offer.
    pub fn workspace_decline_offer(
        &mut self,
        offering_device: &str,
        workspace_id: &str,
    ) -> Result<bool, ClientError> {
        let req = pb::WorkspaceDeclineOfferRequest {
            offering_device: offering_device.to_owned(),
            workspace_id: workspace_id.to_owned(),
        };
        let rep = self
            .rt
            .block_on(self.client.workspace_decline_offer(req))
            .map_err(ClientError::Rpc)?;
        Ok(rep.into_inner().declined)
    }
}
