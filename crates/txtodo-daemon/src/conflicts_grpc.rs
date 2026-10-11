//! `ListConflicts` / `ResolveConflict` (plan M7, design §4.7) and the read-only `Lint` RPC (task
//! `mcp-hygiene-parity`), moved out of `server.rs` for its line budget — the same `impl
//! TxtodoService` extension pattern as `devices_grpc.rs`/`migrate_grpc.rs`. Since ADR 0038
//! (conflict review A) `ListConflicts` sends duplicate groups only and `ResolveConflict` refuses:
//! nothing raises a needs_review flag. Both stay on the wire until the proto drops them.

use crate::convert::status_of;
use crate::duplicates::to_duplicate_group;
use crate::server::TxtodoService;
use tonic::{Request, Response, Status};
use txtodo_proto::v1 as pb;

impl TxtodoService {
    /// One document's duplicate groups; `flags` is always empty (ADR 0038).
    pub(crate) async fn list_conflicts_impl(
        &self,
        r: Request<pb::ConflictsRequest>,
    ) -> Result<Response<pb::ConflictsResponse>, Status> {
        let h = self.actor(&r.get_ref().path)?;
        let found = h.conflicts().await.map_err(status_of)?;
        Ok(Response::new(pb::ConflictsResponse {
            flags: Vec::new(),
            duplicates: found.duplicates.iter().map(to_duplicate_group).collect(),
        }))
    }

    /// Refuses: there is no needs_review flag to resolve since ADR 0038. An older client that
    /// still calls it gets a reason it can show, not an unimplemented RPC.
    pub(crate) async fn resolve_conflict_impl(
        &self,
        _r: Request<pb::ResolveRequest>,
    ) -> Result<Response<pb::ApplyResponse>, Status> {
        Err(Status::failed_precondition(
            "no needs_review flags since ADR 0038: there is nothing to resolve",
        ))
    }

    /// `txtodo lint` for one document: `txtodo_core::lint_findings` over the exact bytes the
    /// daemon holds. Read-only; a missing document is `NOT_FOUND`.
    pub(crate) async fn lint_impl(
        &self,
        r: Request<pb::LintRequest>,
    ) -> Result<Response<pb::LintResponse>, Status> {
        let h = self.actor(&r.get_ref().path)?;
        let contents = h.get().await.map_err(status_of)?;
        let file = txtodo_core::parse_file(&contents.bytes);
        let findings = txtodo_core::lint_findings(&file)
            .into_iter()
            .map(|(line, message)| pb::LintFinding {
                line: u32::try_from(line).unwrap_or(u32::MAX),
                message,
            })
            .collect();
        Ok(Response::new(pb::LintResponse { findings }))
    }
}
