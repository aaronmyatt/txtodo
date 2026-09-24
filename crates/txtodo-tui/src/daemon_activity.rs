//! The activity feed (task `tui-revamp/tui-foundation`), for Settings › Activity. Split out of
//! `daemon.rs` by area.

use txtodo_proto::v1 as pb;

use crate::daemon::{Daemon, DaemonError};

impl Daemon {
    /// One workspace's op log as a stream, newest first then live. `workspace` names another
    /// workspace than this client's own (the feed runs across all of them); `None` is this one.
    pub async fn op_log_stream(
        &mut self,
        workspace: Option<pb::WorkspaceSelector>,
    ) -> Result<tonic::Streaming<pb::OpLogEntry>, DaemonError> {
        let req = pb::OpLogRequest {
            workspace: workspace.or_else(|| self.selector.clone()),
        };
        Ok(self.inner.op_log_stream(req).await?.into_inner())
    }
}
