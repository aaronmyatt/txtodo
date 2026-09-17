//! `SyncStatus` (plan M10, tasks/tui): whitebox on purpose, same shape `pairing_grpc_tests.rs`
//! uses — needs `Workspace::identity_store()` (crate-internal) to register a peer directly,
//! bypassing a real pairing handshake, so these tests stay focused on the RPC's own computation.
//! `device_list_impl`/`DeviceList` have no dedicated test anywhere in this crate today (checked);
//! this is net new coverage for the sibling RPC, not a re-point of an existing suite.

use std::path::Path;
use std::sync::{Arc, RwLock};

use tonic::Request;
use txtodo_model::{DeviceId, Ulid};
use txtodo_proto::v1 as pb;
use txtodo_store::NewDevice;

use crate::clock::{Clock, FakeClock};
use crate::server::TxtodoService;
use crate::workspace::Workspace;

fn touch(p: &Path, bytes: &str) {
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d).unwrap_or_else(|e| panic!("{e}"));
    }
    std::fs::write(p, bytes).unwrap_or_else(|e| panic!("{e}"));
}

fn device(n: u128) -> DeviceId {
    DeviceId::new(Ulid::from_u128(n))
}

/// Registers peer `n` directly on the store (no pairing handshake — `pairing_grpc_tests.rs`
/// already covers that path). `Store::register_device`'s own SQL binds `last_seen` to the same
/// value as `paired_at` at insert time (`identity_store.rs`'s `UPSERT_DEVICE`, `?4` used twice) —
/// so `last_seen_ms` starts equal to `paired_at_ms`, then is frozen there forever: nothing in this
/// crate ever updates it again after registration (see `sync_status_impl`'s own doc comment for
/// the full finding).
fn register_peer(ws: &Workspace, n: u128, paired_at_ms: u64) {
    let mut store = ws.identity_store().lock().unwrap();
    store
        .register_device(&NewDevice {
            device: device(n),
            name: format!("peer-{n}"),
            static_public: [n as u8; 32],
            paired_at_ms,
            last_known_wall_ms: None,
            key_epoch: 0,
        })
        .unwrap();
}

#[tokio::test]
async fn no_peers_is_zero_peers_and_zero_pending() {
    let dir = tempfile::tempdir().unwrap();
    touch(&dir.path().join("todo.txt"), "one\n");
    let clock = Arc::new(FakeClock::new(1_000));
    let ws = Workspace::open(dir.path(), clock as Arc<dyn Clock>).unwrap_or_else(|e| panic!("{e}"));
    let svc = TxtodoService::new(Arc::new(RwLock::new(ws)));

    let resp = svc
        .sync_status_impl(Request::new(pb::SyncStatusRequest { workspace: None }))
        .await
        .unwrap()
        .into_inner();

    assert!(resp.peers.is_empty());
    assert_eq!(resp.pending_ops, 0, "nothing to be pending against");
}

#[tokio::test]
async fn lag_is_now_minus_last_seen_and_ops_since_are_pending() {
    let dir = tempfile::tempdir().unwrap();
    // Two adopted lines mint at least two ops when the workspace first opens, at clock = 1_000.
    touch(&dir.path().join("todo.txt"), "one\ntwo\n");
    let clock = Arc::new(FakeClock::new(1_000));
    let ws = Workspace::open(dir.path(), clock.clone() as Arc<dyn Clock>)
        .unwrap_or_else(|e| panic!("{e}"));
    // Registered (and so last-seen, per register_peer's own doc) at 500 — before the adoption
    // ops' own 1_000 timestamp, so this peer has never seen either of them.
    register_peer(&ws, 1, 500);
    clock.advance_ms(5_500); // now_ms = 1_000 + 5_500 = 6_500
    let svc = TxtodoService::new(Arc::new(RwLock::new(ws)));

    let resp = svc
        .sync_status_impl(Request::new(pb::SyncStatusRequest { workspace: None }))
        .await
        .unwrap()
        .into_inner();

    assert_eq!(resp.peers.len(), 1);
    assert_eq!(
        resp.peers[0].lag_ms, 6_000,
        "6_500 now_ms - 500 last_seen_ms"
    );
    assert!(
        resp.pending_ops >= 2,
        "both adoption ops (at 1_000) committed after this peer's last_seen_ms (500)"
    );
}

#[tokio::test]
async fn removed_and_self_rows_are_excluded_from_peers() {
    let dir = tempfile::tempdir().unwrap();
    touch(&dir.path().join("todo.txt"), "one\n");
    let clock = Arc::new(FakeClock::new(1_000));
    let ws = Workspace::open(dir.path(), clock.clone() as Arc<dyn Clock>)
        .unwrap_or_else(|e| panic!("{e}"));
    register_peer(&ws, 1, 500);
    ws.remove_device(device(1), 2_000).unwrap();
    let svc = TxtodoService::new(Arc::new(RwLock::new(ws)));

    let resp = svc
        .sync_status_impl(Request::new(pb::SyncStatusRequest { workspace: None }))
        .await
        .unwrap()
        .into_inner();

    assert!(
        resp.peers.is_empty(),
        "removed device must not appear as a peer"
    );
}
