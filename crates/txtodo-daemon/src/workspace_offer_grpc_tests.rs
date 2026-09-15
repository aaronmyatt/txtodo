//! `WorkspacePendingOffers`/`WorkspaceAcceptOffer`/`WorkspaceDeclineOffer` over `GlobalService`
//! (task `daemon-workspace-identity-agreement`, stage 6) — whitebox, same-process, mirrors
//! `pairing_grpc_tests.rs`'s own shape: real temp directories and a real `WorkspaceCatalog`, no
//! socket. Offers are seeded directly via `DeviceIdentity::workspace_offers()` (the control
//! channel's own job, stage 5, tested there) so this file stays focused on the gRPC surface alone.

use std::sync::Arc;

use tonic::{Code, Request};
use txtodo_model::{DeviceId, IdentityMode, Ulid};
use txtodo_proto::v1::{self as pb, txtodo_server::Txtodo};
use txtodo_store::WorkspaceId;

use crate::clock::FakeClock;
use crate::device_identity::DeviceIdentity;
use crate::global_service::GlobalService;
use crate::workspace_catalog::{OpenArgs, WorkspaceCatalog};
use crate::workspace_offer_registry::PendingOffer;
use crate::workspace_registry::WorkspaceRegistry;

fn service() -> (tempfile::TempDir, Arc<DeviceIdentity>, GlobalService) {
    let identity_dir = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
    let identity = Arc::new(
        DeviceIdentity::open_in_memory(identity_dir.path(), &FakeClock::new(1_000))
            .unwrap_or_else(|e| panic!("open identity: {e}")),
    );
    let open_args = OpenArgs {
        identity_mode: IdentityMode::Sidecar,
        identity: Arc::clone(&identity),
        relay_url: None,
        device_relay: None,
        relay_dial_peer: None,
        no_lan: true,
        device_file_carrier: None,
    };
    let registry_dir = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
    let registry = WorkspaceRegistry::open(&registry_dir.path().join("registry.db"))
        .unwrap_or_else(|e| panic!("open registry: {e}"));
    let catalog = WorkspaceCatalog::new(registry, open_args, Arc::new(FakeClock::new(1_000)));
    (
        registry_dir,
        identity,
        GlobalService::new(Arc::new(catalog)),
    )
}

fn device(n: u128) -> DeviceId {
    DeviceId::new(Ulid::from_u128(n))
}

fn workspace(n: u128) -> WorkspaceId {
    WorkspaceId::new(Ulid::from_u128(n))
}

#[tokio::test]
async fn pending_offers_lists_what_was_recorded() {
    let (_registry_dir, identity, svc) = service();
    identity
        .workspace_offers()
        .record(PendingOffer {
            offering_device: device(1),
            workspace_id: workspace(1),
            name: "from-peer".to_string(),
            offered_at_ms: 1_000,
        })
        .unwrap();

    let resp = svc
        .workspace_pending_offers(Request::new(pb::WorkspacePendingOffersRequest {}))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(resp.offers.len(), 1);
    assert_eq!(resp.offers[0].offering_device, device(1).to_string());
    assert_eq!(resp.offers[0].workspace_id, workspace(1).to_string());
    assert_eq!(resp.offers[0].name, "from-peer");
}

#[tokio::test]
async fn accept_offer_adopts_it_and_it_stops_being_pending() {
    let (_registry_dir, identity, svc) = service();
    let offered = workspace(2);
    identity
        .workspace_offers()
        .record(PendingOffer {
            offering_device: device(1),
            workspace_id: offered,
            name: "from-peer".to_string(),
            offered_at_ms: 1_000,
        })
        .unwrap();
    let local_dir = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));

    let info = svc
        .workspace_accept_offer(Request::new(pb::WorkspaceAcceptOfferRequest {
            offering_device: device(1).to_string(),
            workspace_id: offered.to_string(),
            local_dir: local_dir.path().display().to_string(),
        }))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(
        info.workspace_id,
        offered.to_string(),
        "the offered id, not a freshly minted one"
    );

    // It now shows up as a real, locally-registered workspace...
    let listed = svc
        .workspace_list(Request::new(pb::WorkspaceListRequest {}))
        .await
        .unwrap()
        .into_inner();
    assert!(
        listed
            .workspaces
            .iter()
            .any(|w| w.workspace_id == offered.to_string())
    );

    // ...and is no longer pending.
    let pending = svc
        .workspace_pending_offers(Request::new(pb::WorkspacePendingOffersRequest {}))
        .await
        .unwrap()
        .into_inner();
    assert!(pending.offers.is_empty());
}

#[tokio::test]
async fn accept_offer_with_no_pending_match_is_not_found() {
    let (_registry_dir, _identity, svc) = service();
    let local_dir = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
    let err = svc
        .workspace_accept_offer(Request::new(pb::WorkspaceAcceptOfferRequest {
            offering_device: device(404).to_string(),
            workspace_id: workspace(404).to_string(),
            local_dir: local_dir.path().display().to_string(),
        }))
        .await
        .unwrap_err();
    assert_eq!(err.code(), Code::NotFound);
}

#[tokio::test]
async fn decline_offer_discards_it_and_reports_false_when_unknown() {
    let (_registry_dir, identity, svc) = service();
    identity
        .workspace_offers()
        .record(PendingOffer {
            offering_device: device(1),
            workspace_id: workspace(3),
            name: "from-peer".to_string(),
            offered_at_ms: 1_000,
        })
        .unwrap();

    let declined = svc
        .workspace_decline_offer(Request::new(pb::WorkspaceDeclineOfferRequest {
            offering_device: device(1).to_string(),
            workspace_id: workspace(3).to_string(),
        }))
        .await
        .unwrap()
        .into_inner()
        .declined;
    assert!(declined);

    let pending = svc
        .workspace_pending_offers(Request::new(pb::WorkspacePendingOffersRequest {}))
        .await
        .unwrap()
        .into_inner();
    assert!(pending.offers.is_empty());

    let again = svc
        .workspace_decline_offer(Request::new(pb::WorkspaceDeclineOfferRequest {
            offering_device: device(1).to_string(),
            workspace_id: workspace(3).to_string(),
        }))
        .await
        .unwrap()
        .into_inner()
        .declined;
    assert!(!again, "already consumed");
}
