//! The catalog's offer half (task `daemon-workspace-identity-agreement`): the pending-offer surface
//! (mirrored on its own since task `remote-workspace-mirror`, see `workspace_catalog_mirror.rs`).
//! Pairing no longer re-keys a joiner's folder to the initiator's workspace id (task
//! `two-kinds-of-workspace`): a list's id is its name's and a repo is not synced. Same `impl
//! WorkspaceCatalog`, split from `workspace_catalog.rs` for its file budget.

use crate::workspace_catalog::WorkspaceCatalog;
use tonic::Status;
use txtodo_model::DeviceId;
use txtodo_store::WorkspaceId;

impl WorkspaceCatalog {
    /// `WorkspacePendingOffers` RPC (task `daemon-workspace-identity-agreement`, stage 6): every
    /// workspace a peer has offered over the control channel (stage 5) that this device has not
    /// yet accepted or declined.
    pub fn pending_offers(&self) -> Vec<crate::workspace_offer_registry::PendingOffer> {
        self.open_args.identity.workspace_offers().list()
    }

    /// `WorkspaceAcceptOffer` RPC: mirrors the pending offer's workspace now, the same way the
    /// mirror task would on its own (task `remote-workspace-mirror`: always under
    /// `<remote root>/<workspace-id>/`, never a directory the caller picks). Consumes the pending
    /// offer whether mirroring succeeds or fails — a human who explicitly acted on an offer should
    /// never see it silently reappear as still-pending.
    pub fn accept_offer(
        &self,
        offering_device: DeviceId,
        workspace_id: WorkspaceId,
    ) -> Result<crate::workspace_registry::WorkspaceEntry, Status> {
        let offer = self
            .open_args
            .identity
            .workspace_offers()
            .take(offering_device, workspace_id);
        let Some(offer) = offer else {
            return Err(Status::not_found(format!(
                "no pending offer for workspace {workspace_id} from device {offering_device}"
            )));
        };
        if crate::list_workspace::is_list_id(workspace_id) {
            return self.mirror_offered_list(&offer)?.ok_or_else(|| {
                Status::failed_precondition("only your own devices can offer you a list")
            });
        }
        self.mirror_workspace(workspace_id)
    }

    /// `WorkspaceDeclineOffer` RPC: discards a pending offer without adopting it. `false` when no
    /// such pending offer was found.
    pub fn decline_offer(&self, offering_device: DeviceId, workspace_id: WorkspaceId) -> bool {
        self.open_args
            .identity
            .workspace_offers()
            .decline(offering_device, workspace_id)
    }
}
