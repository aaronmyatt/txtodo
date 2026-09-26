//! `Daemon`'s `SyncStatus` RPC (task sync-drift line 7: `txtodo doctor`'s `sync` rows), split out
//! of `client.rs` for its file-length budget, the same pattern as `client_identity.rs`.

use crate::client::{ClientError, Daemon};
use txtodo_proto::v1::{self as pb};

impl Daemon {
    /// Every paired peer's lag, where its incoming ops keep being refused, and whether the daemon
    /// has parked it for holding no key we share.
    pub fn sync_status(&mut self) -> Result<pb::SyncStatusResponse, ClientError> {
        let req = pb::SyncStatusRequest {
            workspace: self.selector.clone(),
        };
        let rep = self
            .rt
            .block_on(self.client.sync_status(req))
            .map_err(ClientError::Rpc)?;
        Ok(rep.into_inner())
    }
}
