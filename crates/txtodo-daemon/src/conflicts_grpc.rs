//! `ListConflicts` / `ResolveConflict` (plan M7, design §4.7) and the read-only `Lint` RPC (task
//! `mcp-hygiene-parity`), moved out of `server.rs` for its line budget — the same `impl
//! TxtodoService` extension pattern as `devices_grpc.rs`/`migrate_grpc.rs`. Same-word review
//! (`needs_review` flags) went with the todo.txt mirror (ADR 0038): `ListConflicts` sends duplicate
//! groups only, and `ResolveConflict` stays on the wire only until the proto drops it.

use crate::convert::status_of;
use crate::duplicates::to_duplicate_group;
use crate::server::TxtodoService;
use tonic::{Request, Response, Status};
use txtodo_proto::v1 as pb;

impl TxtodoService {
    /// One document's duplicate groups (ADR 0032). `flags` is always empty; old flag rows in the
    /// store are never read.
    pub(crate) async fn list_conflicts_impl(
        &self,
        r: Request<pb::ConflictsRequest>,
    ) -> Result<Response<pb::ConflictsResponse>, Status> {
        let h = self.actor(&r.get_ref().path)?;
        let groups = h.conflicts().await.map_err(status_of)?;
        Ok(Response::new(pb::ConflictsResponse {
            flags: Vec::new(),
            duplicates: groups.iter().map(to_duplicate_group).collect(),
        }))
    }

    /// Always `Unimplemented`: there is no flag left to resolve (ADR 0038). A duplicate copy goes
    /// with an ordinary `Apply` `Delete`.
    /// Ref: <https://grpc.io/docs/guides/status-codes/>
    pub(crate) async fn resolve_conflict_impl(
        &self,
        _r: Request<pb::ResolveRequest>,
    ) -> Result<Response<pb::ApplyResponse>, Status> {
        Err(Status::unimplemented(
            "same-word conflict review was removed (ADR 0038); nothing to resolve",
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
