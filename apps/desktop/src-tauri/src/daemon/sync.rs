//! `DaemonClient::sync_status` (task sync-divergence-check/protocol-mismatch), split out of
//! `daemon.rs` for its file budget, like `workspace.rs`.

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
}
