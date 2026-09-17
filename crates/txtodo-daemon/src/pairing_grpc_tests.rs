//! Round trip: `pair_offer` → `pair_accept` → `pair_confirm_sas` across two `TxtodoService`
//! instances, using `PairingRegistry`'s relay-seam methods for the daemon-to-daemon leg that has
//! no transport yet (`pairing_grpc.rs`'s module doc explains why). Whitebox on purpose: the group
//! key must never appear on the wire, so verifying it landed needs `Workspace::key_store()`, which
//! only crate-internal code can reach — not a `tests/grpc.rs`-style external integration test.

use std::path::Path;
use std::sync::{Arc, RwLock};

use tonic::Request;
use txtodo_proto::v1::txtodo_server::Txtodo;
use txtodo_proto::v1::{self as pb};
use txtodo_sync::{GroupId, KeyId, PAIRING_WINDOW_MS};

use crate::clock::{Clock, FakeClock};
use crate::pairing_wire::response_to_code;
use crate::server::TxtodoService;
use crate::workspace::Workspace;

fn service(dir: &Path, clock: Arc<FakeClock>) -> TxtodoService {
    let ws = Workspace::open(dir, clock as Arc<dyn Clock>).unwrap_or_else(|e| panic!("open: {e}"));
    TxtodoService::new(Arc::new(RwLock::new(ws)))
}

async fn offer(svc: &TxtodoService) -> pb::PairOfferResponse {
    svc.pair_offer(Request::new(pb::PairOfferRequest { workspace: None }))
        .await
        .unwrap()
        .into_inner()
}

async fn accept(svc: &TxtodoService, code: String) -> pb::PairResult {
    svc.pair_accept(Request::new(pb::PairAcceptRequest {
        code,
        workspace: None,
    }))
    .await
    .unwrap()
    .into_inner()
}

async fn confirm(svc: &TxtodoService) -> Result<pb::PairResult, tonic::Status> {
    svc.pair_confirm_sas(Request::new(pb::PairConfirmRequest { workspace: None }))
        .await
        .map(tonic::Response::into_inner)
}

#[tokio::test]
async fn qr_payload_has_no_field_beyond_the_documented_nine() {
    let dir = tempfile::tempdir().unwrap();
    let svc = service(dir.path(), Arc::new(FakeClock::new(1_000)));
    let response = offer(&svc).await;

    let code = response_to_code(&response);
    let value: serde_json::Value = serde_json::from_str(&code).unwrap();
    let mut keys: Vec<&str> = value
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        [
            "device",
            "endpoint",
            "group_id",
            "identity_mode",
            "nonce",
            "relay_node_id",
            "relay_url",
            "workspace_id",
            "x25519_pub"
        ]
    );
    assert!(!response.x25519_pub.is_empty());
    assert!(!response.nonce.is_empty());
    // Sidecar is the daemon's own default (docs/questions.md Q2) for a brand-new workspace.
    assert_eq!(response.identity_mode, "sidecar");
    // No relay configured on this fresh test workspace (plan M8 sync-pairing-relay): both relay
    // rendezvous fields are empty, not merely absent — the no-regression case notes.md requires.
    assert_eq!(response.relay_node_id, "");
    assert_eq!(response.relay_url, "");
}

#[tokio::test]
async fn pair_offer_refuses_a_second_concurrent_pairing() {
    let dir = tempfile::tempdir().unwrap();
    let svc = service(dir.path(), Arc::new(FakeClock::new(1_000)));
    offer(&svc).await;

    let err = svc
        .pair_offer(Request::new(pb::PairOfferRequest { workspace: None }))
        .await
        .unwrap_err();
    assert_eq!(err.code(), tonic::Code::ResourceExhausted);
}

#[tokio::test]
async fn pairing_window_expiry_surfaces_and_then_frees_the_slot() {
    let dir = tempfile::tempdir().unwrap();
    let clock = Arc::new(FakeClock::new(1_000));
    let svc = service(dir.path(), Arc::clone(&clock));
    offer(&svc).await;

    clock.advance_ms(PAIRING_WINDOW_MS + 1);
    let err = confirm(&svc).await.unwrap_err();
    assert_eq!(err.code(), tonic::Code::FailedPrecondition);
    // The stale slot was let go, not left jammed forever: a fresh offer now succeeds.
    svc.pair_offer(Request::new(pb::PairOfferRequest { workspace: None }))
        .await
        .unwrap();
}

/// Drives the handshake to a matching SAS on both sides via real gRPC calls, relaying only what
/// has no transport yet (see `pairing_grpc.rs`'s module doc): B's public key back to A. Neither
/// side has called `pair_confirm_sas` yet when this returns.
async fn handshake(a: &TxtodoService, b: &TxtodoService, now_ms: u64) {
    let offer_a = offer(a).await;
    let code = response_to_code(&offer_a);
    accept(b, code).await;

    let peer_public = b.workspace().pairing().joiner_public_key(now_ms).unwrap();
    let peer_device = b.workspace().device();
    a.workspace()
        .pairing()
        .complete_as_initiator(peer_device, peer_public, now_ms)
        .unwrap();
}

/// [`handshake`], then both sides call `pair_confirm_sas` for real and are asserted to derive the
/// same SAS — but neither has *learned* of the other's confirmation yet (see `mark_remote_confirmed`).
/// `pub(crate)`: reused by `lan_session_tests`'s `no_secrets_appear_in_logs_across_a_real_pair_and_sync`,
/// which needs a real pairing round rather than a hand-seeded group key (`security-m4-review`).
pub(crate) async fn handshake_and_confirm(a: &TxtodoService, b: &TxtodoService, now_ms: u64) {
    handshake(a, b, now_ms).await;
    let sas_a = confirm(a).await.unwrap().sas;
    let sas_b = confirm(b).await.unwrap().sas;
    assert_eq!(sas_a, sas_b, "both sides derive the same SAS");
}

/// Both sides have called `pair_confirm_sas` but neither has learned of the other's confirmation
/// (no transport): `try_finalize_initiator` reports not-ready. Then relays both confirmations and
/// finishes the exchange for real, returning the group id both sides now hold.
/// `pub(crate)`: see [`handshake_and_confirm`]'s doc for why.
pub(crate) async fn finalize_after_both_confirm(
    a: &TxtodoService,
    b: &TxtodoService,
    now_ms: u64,
) -> GroupId {
    let group = a.workspace().group();
    let ready = |ws: &TxtodoService| {
        ws.workspace()
            .pairing()
            .try_finalize_initiator(
                ws.workspace().key_store().as_ref(),
                ws.workspace().device_static_public(),
                now_ms,
            )
            .unwrap()
    };
    assert!(ready(a).is_none());

    a.workspace()
        .pairing()
        .mark_remote_confirmed(now_ms)
        .unwrap();
    b.workspace()
        .pairing()
        .mark_remote_confirmed(now_ms)
        .unwrap();

    let sealed = ready(a).expect("both sides confirmed: the initiator now wraps its group key");
    b.workspace()
        .adopt_group_key(group, &sealed, now_ms)
        .unwrap();
    group
}

#[tokio::test]
async fn full_round_trip_converges_only_after_both_sides_confirm() {
    let dir_a = tempfile::tempdir().unwrap();
    let dir_b = tempfile::tempdir().unwrap();
    let clock = Arc::new(FakeClock::new(1_000));
    let a = service(dir_a.path(), Arc::clone(&clock));
    let b = service(dir_b.path(), Arc::clone(&clock));
    let now_ms = clock.now_ms();

    handshake_and_confirm(&a, &b, now_ms).await;
    let group = finalize_after_both_confirm(&a, &b, now_ms).await;

    let key_a = a
        .workspace()
        .key_store()
        .get(KeyId::Group(0))
        .unwrap()
        .unwrap();
    let key_b = b
        .workspace()
        .key_store()
        .get(KeyId::Group(0))
        .unwrap()
        .unwrap();
    assert_eq!(
        key_a.expose(),
        key_b.expose(),
        "both sides hold the same key"
    );
    assert_eq!(
        b.workspace().group(),
        group,
        "the joiner adopted the group it joined"
    );
}

#[tokio::test]
async fn pairing_registers_the_initiators_static_public_key_in_the_joiners_devices_table() {
    let dir_a = tempfile::tempdir().unwrap();
    let dir_b = tempfile::tempdir().unwrap();
    let clock = Arc::new(FakeClock::new(1_000));
    let a = service(dir_a.path(), Arc::clone(&clock));
    let b = service(dir_b.path(), Arc::clone(&clock));
    let now_ms = clock.now_ms();

    handshake_and_confirm(&a, &b, now_ms).await;
    finalize_after_both_confirm(&a, &b, now_ms).await;

    // Plan M4 tasks/sync-device-remove, ADR 0021: the joiner registers the initiator's long-term
    // static public key in the shared device-global `devices` table — the persistence rotation
    // is gated on.
    let b_ws = b.workspace();
    let b_store = b_ws.identity_store().lock().unwrap();
    let devices = b_store.list_devices().unwrap();
    assert_eq!(devices.len(), 1, "the joiner learned exactly one peer");
    assert_eq!(devices[0].device, a.workspace().device());
    assert_eq!(
        devices[0].static_public,
        a.workspace().device_static_public().to_bytes()
    );
    assert!(devices[0].removed_at_ms.is_none());
}

#[tokio::test]
async fn one_sided_confirmation_lands_no_key_on_either_side() {
    let dir_a = tempfile::tempdir().unwrap();
    let dir_b = tempfile::tempdir().unwrap();
    let clock = Arc::new(FakeClock::new(1_000));
    let a = service(dir_a.path(), Arc::clone(&clock));
    let b = service(dir_b.path(), Arc::clone(&clock));
    let now_ms = clock.now_ms();

    handshake(&a, &b, now_ms).await;
    // Only A's human taps confirm; B's never does, so no "B confirmed" signal ever exists for a
    // relay to carry to A either — the honest one-sided case (not a relay bug).
    confirm(&a).await.unwrap();

    let sealed = a
        .workspace()
        .pairing()
        .try_finalize_initiator(
            a.workspace().key_store().as_ref(),
            a.workspace().device_static_public(),
            now_ms,
        )
        .unwrap();
    assert!(sealed.is_none(), "one-sided confirmation transfers no key");
    assert!(
        a.workspace()
            .key_store()
            .get(KeyId::Group(0))
            .unwrap()
            .is_none()
    );

    let group = GroupId(0);
    assert!(b.workspace().adopt_group_key(group, &[], now_ms).is_err());
}
