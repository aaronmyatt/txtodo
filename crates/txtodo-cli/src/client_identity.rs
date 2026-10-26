//! `Daemon`'s identity-mode RPC (tasks/sidecar-migrate-tagged), split out of `client.rs` for its
//! file-length budget, the same pattern as `client_workspace.rs`.

use crate::client::{ClientError, Daemon};
use txtodo_proto::v1::{self as pb};

impl Daemon {
    /// Converts this workspace to Sidecar identity (ADR 0019); `dry_run` only counts.
    pub fn migrate_identity(
        &mut self,
        dry_run: bool,
    ) -> Result<pb::MigrateIdentityResponse, ClientError> {
        let req = pb::MigrateIdentityRequest {
            dry_run,
            workspace: self.selector.clone(),
        };
        let rep = self
            .rt
            .block_on(self.client.migrate_identity(req))
            .map_err(ClientError::Rpc)?;
        Ok(rep.into_inner())
    }
}
