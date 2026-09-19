//! The catalog's offer/adoption half (tasks `daemon-workspace-identity-agreement`,
//! `pairing-workspace-identity`): the pending-offer surface a human accepts or declines, and the id
//! rekeying a joiner needs once pairing hands it the initiator's workspace id. Same `impl
//! WorkspaceCatalog`, split from `workspace_catalog.rs` for its file budget.

use crate::server::SharedWorkspace;
use crate::workspace_catalog::WorkspaceCatalog;
use std::path::Path;
use std::sync::{Arc, PoisonError};
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

    /// `WorkspaceAcceptOffer` RPC: adopts the pending offer's workspace id verbatim into the local
    /// registry at `local_dir` (`WorkspaceRegistry::adopt`'s own collision guards apply). Consumes
    /// the pending offer whether adoption succeeds or fails — a human who explicitly acted on an
    /// offer should never see it silently reappear as still-pending.
    pub fn accept_offer(
        &self,
        offering_device: DeviceId,
        workspace_id: WorkspaceId,
        local_dir: &Path,
    ) -> Result<crate::workspace_registry::WorkspaceEntry, Status> {
        let offer = self
            .open_args
            .identity
            .workspace_offers()
            .take(offering_device, workspace_id);
        if offer.is_none() {
            return Err(Status::not_found(format!(
                "no pending offer for workspace {workspace_id} from device {offering_device}"
            )));
        }
        {
            let mut registry = self.registry.lock().unwrap_or_else(PoisonError::into_inner);
            registry
                .adopt(workspace_id, local_dir, self.clock.as_ref())
                .map_err(|e| Status::invalid_argument(format!("accept {workspace_id}: {e}")))?;
        }
        let registry = self.registry.lock().unwrap_or_else(PoisonError::into_inner);
        registry
            .get(workspace_id)
            .map_err(|e| Status::internal(format!("look up workspace {workspace_id}: {e}")))?
            .ok_or_else(|| {
                Status::internal(format!("workspace {workspace_id} vanished after adopting"))
            })
    }

    /// The registry half of [`Self::adopt_offered_workspace_id`], split out for that function's
    /// cognitive-complexity budget: releases `current_id`'s row for `root`, then adopts
    /// `offered_id` for it — rolling back to `current_id` (logged, never silent) if `offered_id`
    /// turns out to already name a different root on this device.
    fn rekey_registry(
        &self,
        current_id: WorkspaceId,
        offered_id: WorkspaceId,
        root: &Path,
    ) -> Result<(), Status> {
        let mut registry = self.registry.lock().unwrap_or_else(PoisonError::into_inner);
        registry
            .remove(current_id, self.clock.as_ref())
            .map_err(|e| {
                Status::internal(format!(
                    "release {current_id} before adopting {offered_id}: {e}"
                ))
            })?;
        if let Err(e) = registry.adopt(offered_id, root, self.clock.as_ref()) {
            if let Err(rollback_err) = registry.adopt(current_id, root, self.clock.as_ref()) {
                tracing::error!(
                    %current_id, %offered_id, error = %rollback_err,
                    "workspace_id_rekey_rollback_failed"
                );
            }
            return Err(Status::invalid_argument(format!(
                "pairing offered workspace {offered_id}, which is already registered to a \
                 different directory on this device: {e}"
            )));
        }
        Ok(())
    }

    /// `PairAccept`'s own id adoption (task `pairing-workspace-identity`, distinct from
    /// `accept_offer` above): the joiner's daemon already self-registered and opened `ws` under a
    /// locally-minted id (`open_one`, run unconditionally at daemon startup for the `--dir` bridge,
    /// long before any pairing RPC) — first-registrant-wins means the joiner must give that id up
    /// in favor of the initiator's `offered_id` instead. A no-op returning `current_id` when the
    /// two already match. Never touches `root/.txtodo/` — only the registry row's id, this
    /// process's `open` map key, `ws`'s own live id, and the device-level relay/file-carrier route
    /// tables change. Refuses (never silently substitutes) only when `offered_id` is already
    /// actively registered locally under a *different* root — the one collision `adopt` cannot
    /// resolve by itself; the far more common case, `ws`'s own root being self-registered under
    /// its old id, is exactly what this releases first.
    pub fn adopt_offered_workspace_id(
        &self,
        ws: &SharedWorkspace,
        offered_id: WorkspaceId,
    ) -> Result<WorkspaceId, Status> {
        let (current_id, root) = {
            let guard = ws.read().unwrap_or_else(PoisonError::into_inner);
            (guard.workspace_id(), guard.root().to_path_buf())
        };
        if current_id == offered_id {
            return Ok(current_id);
        }
        self.rekey_registry(current_id, offered_id, &root)?;
        {
            let mut open = self.open.write().unwrap_or_else(PoisonError::into_inner);
            if let Some(mut opened) = open.remove(&current_id) {
                opened.rekey(offered_id);
                open.insert(offered_id, opened);
            }
            self.slots.rekey(current_id, offered_id);
        }
        ws.read()
            .unwrap_or_else(PoisonError::into_inner)
            .set_workspace_id(offered_id);
        for routes in [
            self.open_args
                .device_relay
                .as_ref()
                .map(Arc::as_ref)
                .map(crate::device_relay::DeviceRelay::routes),
            self.open_args
                .device_file_carrier
                .as_ref()
                .map(Arc::as_ref)
                .map(crate::file_carrier::DeviceFileCarrier::routes),
        ]
        .into_iter()
        .flatten()
        {
            routes.unregister(current_id);
            crate::workspace_catalog_open::register_route(ws, offered_id, Some(routes));
        }
        Ok(offered_id)
    }

    /// `WorkspaceDeclineOffer` RPC: discards a pending offer without adopting it. `false` when no
    /// such pending offer was found.
    pub fn decline_offer(&self, offering_device: DeviceId, workspace_id: WorkspaceId) -> bool {
        self.open_args
            .identity
            .workspace_offers()
            .take(offering_device, workspace_id)
            .is_some()
    }
}
