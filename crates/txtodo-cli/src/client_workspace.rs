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
}
