//! `WorkspacePendingOffers`/`WorkspaceAcceptOffer`/`WorkspaceDeclineOffer` (task
//! `daemon-workspace-identity-agreement`, stage 6) — the `GlobalService` half of these RPCs lives
//! here rather than growing `global_service.rs`'s own file, mirroring `progress.rs`/`notes.rs`'s
//! `impl TxtodoService` extension pattern (this crate cannot split a single `impl Txtodo for
//! GlobalService` block across two files — that is a coherence error, not a style choice — so
//! `global_service.rs`'s own trait impl still has the three method *signatures*, each a one-line
//! delegate into the free functions below). `#[tonic::async_trait]` rewrites every trait method's
//! signature to an explicit boxed-future form; a `macro_rules!`-generated `async fn` sidesteps
//! that rewrite (the attribute macro sees an unexpanded macro call, not an `async fn`, so it never
//! touches it) and fails to match the trait — tried and reverted while building this, noted so it
//! isn't retried: the bare `TxtodoService` stub methods for these RPCs stay written out by hand in
//! `server.rs`, same as its existing `WorkspaceAdd`/`Remove`/`List` stubs.

use tonic::{Request, Response, Status};
use txtodo_model::{DeviceId, Ulid};
use txtodo_proto::v1::{self as pb};
use txtodo_store::WorkspaceId;

use crate::global_service::{GlobalService, to_workspace_info};
use crate::workspace_offer_registry::PendingOffer;

fn parse_device_id(text: &str) -> Result<DeviceId, Status> {
    Ulid::parse(text)
        .map(DeviceId::new)
        .ok_or_else(|| Status::invalid_argument(format!("{text:?} is not a ULID")))
}

fn parse_workspace_id(text: &str) -> Result<WorkspaceId, Status> {
    Ulid::parse(text)
        .map(WorkspaceId::new)
        .ok_or_else(|| Status::invalid_argument(format!("{text:?} is not a ULID")))
}

fn to_pending_offer(o: PendingOffer) -> pb::PendingWorkspaceOffer {
    pb::PendingWorkspaceOffer {
        offering_device: o.offering_device.to_string(),
        workspace_id: o.workspace_id.to_string(),
        name: o.name,
        offered_at_ms: o.offered_at_ms,
    }
}

pub(crate) async fn pending_offers(
    service: &GlobalService,
    _r: Request<pb::WorkspacePendingOffersRequest>,
) -> Result<Response<pb::WorkspacePendingOffersResponse>, Status> {
    let offers = service
        .catalog()
        .pending_offers()
        .into_iter()
        .map(to_pending_offer)
        .collect();
    Ok(Response::new(pb::WorkspacePendingOffersResponse { offers }))
}

pub(crate) async fn accept_offer(
    service: &GlobalService,
    r: Request<pb::WorkspaceAcceptOfferRequest>,
) -> Result<Response<pb::WorkspaceInfo>, Status> {
    let req = r.into_inner();
    let offering_device = parse_device_id(&req.offering_device)?;
    let workspace_id = parse_workspace_id(&req.workspace_id)?;
    let entry = service.catalog().accept_offer(
        offering_device,
        workspace_id,
        std::path::Path::new(&req.local_dir),
    )?;
    Ok(Response::new(to_workspace_info(entry)))
}

pub(crate) async fn decline_offer(
    service: &GlobalService,
    r: Request<pb::WorkspaceDeclineOfferRequest>,
) -> Result<Response<pb::WorkspaceDeclineOfferResponse>, Status> {
    let req = r.into_inner();
    let offering_device = parse_device_id(&req.offering_device)?;
    let workspace_id = parse_workspace_id(&req.workspace_id)?;
    let declined = service
        .catalog()
        .decline_offer(offering_device, workspace_id);
    Ok(Response::new(pb::WorkspaceDeclineOfferResponse {
        declined,
    }))
}
