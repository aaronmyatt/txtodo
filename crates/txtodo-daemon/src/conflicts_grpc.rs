//! `ListConflicts` / `ResolveConflict` (plan M7, design §4.7) and the read-only `Lint` RPC (task
//! `mcp-hygiene-parity`), moved out of `server.rs` for its line budget — the same `impl
//! TxtodoService` extension pattern as `devices_grpc.rs`/`migrate_grpc.rs`.

use crate::convert::{
    applied_of, parse_principal, parse_resolution, parse_task_ref, status_of, to_flag,
};
use crate::server::TxtodoService;
use tonic::{Request, Response, Status};
use txtodo_proto::v1 as pb;

impl TxtodoService {
    /// The open `needs_review` flags of one document.
    pub(crate) async fn list_conflicts_impl(
        &self,
        r: Request<pb::ConflictsRequest>,
    ) -> Result<Response<pb::ConflictsResponse>, Status> {
        let h = self.actor(&r.get_ref().path)?;
        let rows = h.conflicts().await.map_err(status_of)?;
        Ok(Response::new(pb::ConflictsResponse {
            flags: rows.iter().map(to_flag).collect(),
        }))
    }

    /// Resolves one flag, keeping mine, theirs or the merged text.
    pub(crate) async fn resolve_conflict_impl(
        &self,
        r: Request<pb::ResolveRequest>,
    ) -> Result<Response<pb::ApplyResponse>, Status> {
        let req = r.into_inner();
        let h = self.actor(&req.path)?;
        let task = parse_task_ref(req.task)?;
        let resolution = parse_resolution(req.resolution)?;
        // Unset agent = the user on this device; an MCP agent's resolution is attributed to it.
        let principal = parse_principal(req.agent, self.workspace().device())?;
        let a = h
            .resolve(task, resolution, principal)
            .await
            .map_err(status_of)?;
        Ok(Response::new(applied_of(a)))
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
