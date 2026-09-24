//! The op log's history and the daemon's `Undo` (task `tui-revamp/tui-foundation`): list mode's
//! `u` and a toast's Undo go through the daemon, never a local re-toggle. Split out of `daemon.rs`
//! by area.

use txtodo_proto::v1 as pb;

use crate::daemon::{Daemon, DaemonError};

impl Daemon {
    /// Ops newest first for `path` (every document when empty), at most `limit` (0: the daemon's
    /// default).
    pub async fn history(
        &mut self,
        path: &str,
        limit: u32,
    ) -> Result<pb::HistoryResponse, DaemonError> {
        let req = pb::HistoryRequest {
            path: path.to_owned(),
            limit,
            workspace: self.selector.clone(),
            ..pb::HistoryRequest::default()
        };
        Ok(self.inner.history(req).await?.into_inner())
    }

    /// Undoes the newest `steps` changes to `path` (0 means 1).
    pub async fn undo(&mut self, path: &str, steps: u32) -> Result<pb::ApplyResponse, DaemonError> {
        let req = pb::UndoRequest {
            path: path.to_owned(),
            steps,
            workspace: self.selector.clone(),
        };
        Ok(self.inner.undo(req).await?.into_inner())
    }
}
