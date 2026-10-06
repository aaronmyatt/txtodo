//! `DaemonClient::sync_status` (task sync-divergence-check/protocol-mismatch) and the paired
//! device list/remove (Settings > Devices, task desktop-ui-revamp), split out of `daemon.rs` for
//! its file budget, like `workspace.rs`.

use super::{DaemonClient, DaemonError};
use txtodo_proto::v1 as pb;

impl DaemonClient {
    /// Paired peers' sync state as the daemon sees it: lag, stuck files, parked, and the sync
    /// protocol a peer speaks when it is not ours (then nothing syncs with it).
    pub async fn sync_status(&mut self) -> Result<pb::SyncStatusResponse, DaemonError> {
        let req = pb::SyncStatusRequest {
            workspace: self.selector.clone(),
        };
        Ok(self.inner.sync_status(req).await?.into_inner())
    }

    /// Devices paired into this workspace's sync group, removed ones included (tombstoned).
    pub async fn device_list(&mut self) -> Result<pb::DeviceListResponse, DaemonError> {
        let req = pb::DeviceListRequest {
            workspace: self.selector.clone(),
        };
        Ok(self.inner.device_list(req).await?.into_inner())
    }

    /// Removes a device and rotates the group key to the rest; refuses this device and the last.
    pub async fn device_remove(
        &mut self,
        id: String,
    ) -> Result<pb::DeviceRemoveResponse, DaemonError> {
        let req = pb::DeviceRemoveRequest {
            id,
            workspace: self.selector.clone(),
        };
        Ok(self.inner.device_remove(req).await?.into_inner())
    }
}
