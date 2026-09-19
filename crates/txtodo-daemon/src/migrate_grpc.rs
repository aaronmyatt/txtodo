//! `MigrateIdentity` (tasks/sidecar-migrate-tagged, ADR 0019): the RPC over
//! `workspace_migrate.rs`. Delegated to from `server.rs`, the same split as `devices_grpc.rs`.
//!
//! The workspace lock is held only to flip the mode and list the actors; the per-document round
//! trips run after it is released, so every other RPC keeps being served while a large workspace
//! migrates.

use std::sync::PoisonError;

use crate::server::TxtodoService;
use crate::workspace_migrate::migrate_documents;
use tonic::{Request, Response, Status};
use txtodo_model::IdentityMode;
use txtodo_proto::v1 as pb;

impl TxtodoService {
    /// Converts the workspace to Sidecar identity, or with `dry_run` only counts what would change.
    pub(crate) async fn migrate_identity_impl(
        &self,
        r: Request<pb::MigrateIdentityRequest>,
    ) -> Result<Response<pb::MigrateIdentityResponse>, Status> {
        let dry_run = r.get_ref().dry_run;
        let (was_tagged, handles) = if dry_run {
            let ws = self.workspace();
            (
                ws.identity_mode() == IdentityMode::Tagged,
                ws.document_handles(),
            )
        } else {
            let shared = self.shared_workspace();
            let mut ws = shared.write().unwrap_or_else(PoisonError::into_inner);
            let was_tagged = ws.identity_mode() == IdentityMode::Tagged;
            let handles = ws
                .begin_sidecar_migration()
                .map_err(|e| Status::internal(format!("cannot switch to sidecar identity: {e}")))?;
            (was_tagged, handles)
        };
        let report = migrate_documents(handles, dry_run).await;
        let count = |n: usize| u32::try_from(n).unwrap_or(u32::MAX);
        Ok(Response::new(pb::MigrateIdentityResponse {
            files: count(report.files),
            tasks: count(report.tasks),
            stripped: count(report.stripped),
            failures: report
                .failures
                .into_iter()
                .map(|(path, why)| format!("{path}: {why}"))
                .collect(),
            was_tagged,
            renumbered: count(report.renumbered),
        }))
    }
}
