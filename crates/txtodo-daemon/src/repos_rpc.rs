//! `Repos` (task `two-kinds-of-workspace`, ADR 0043): the folders the daemon scans for git clones
//! that hold a todo list, and the scan. Device-level like the registry RPCs. The roots change
//! first, then a scan starts if asked, in the background unless `wait`; the answer is the state.
//! Ref: <https://docs.rs/tokio/latest/tokio/task/fn.spawn_blocking.html>

use std::sync::Arc;

use tonic::{Request, Response, Status};
use txtodo_proto::v1 as pb;

use crate::global_service::GlobalService;

/// `Repos`.
pub(crate) async fn repos(
    service: &GlobalService,
    r: Request<pb::ReposRequest>,
) -> Result<Response<pb::ReposResponse>, Status> {
    let req = r.into_inner();
    let catalog = Arc::clone(service.catalog());
    catalog.edit_scan_roots(&req.add_roots, &req.remove_roots)?;
    if req.scan
        && let Some(guard) = catalog.try_begin_scan()
    {
        let scanning = Arc::clone(&catalog);
        let task = tokio::task::spawn_blocking(move || scanning.run_scan(guard));
        if req.wait {
            task.await
                .map_err(|e| Status::internal(format!("scan task failed: {e}")))??;
        }
    }
    let report = catalog.scan_report();
    Ok(Response::new(pb::ReposResponse {
        roots: catalog.scan_roots()?,
        last_scan_ms: catalog.last_scan_ms(),
        running: catalog.scan_running(),
        found: report.found,
        registered: report.registered,
    }))
}
