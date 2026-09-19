//! `MigrateIdentity` and per-task history helpers (tasks/sidecar-migrate-tagged), split out of
//! `mod.rs` for its file-length budget. A child module, so it reads `Daemon`'s private client.

use super::Daemon;
use txtodo_proto::v1::{self as pb};

impl Daemon {
    /// Calls `MigrateIdentity` on the sole open workspace.
    pub async fn migrate_identity(&mut self, dry_run: bool) -> pb::MigrateIdentityResponse {
        let req = pb::MigrateIdentityRequest {
            dry_run,
            workspace: None,
        };
        self.client
            .migrate_identity(req)
            .await
            .unwrap_or_else(|e| panic!("migrate_identity: {e}"))
            .into_inner()
    }

    /// Every op recorded for one task, oldest first — the whole log, not just since the baseline.
    pub async fn task_history(&mut self, task_id: &str) -> Vec<pb::OpSummary> {
        let req = pb::HistoryRequest {
            path: "todo.txt".into(),
            task_id: task_id.to_owned(),
            limit: 1000,
            before_seq: 0,
            workspace: None,
        };
        let mut ops = self
            .client
            .history(req)
            .await
            .unwrap_or_else(|e| panic!("history: {e}"))
            .into_inner()
            .ops;
        ops.reverse();
        ops
    }
}
