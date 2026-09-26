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

/// Task sync-drift line 7: a peer whose run keeps being refused says where and why, and a peer
/// parked for holding no key we share (line 5) says so.
#[tokio::test]
async fn a_peer_says_where_its_sync_is_stuck_and_whether_it_is_parked() {
    let dir = tempfile::tempdir().unwrap();
    touch(&dir.path().join("todo.txt"), "one\n");
    let clock = Arc::new(FakeClock::new(1_000));
    let ws = Workspace::open(dir.path(), clock as Arc<dyn Clock>).unwrap_or_else(|e| panic!("{e}"));
    register_peer(&ws, 1, 500);
    register_peer(&ws, 2, 500);
    let file = txtodo_model::FilePath::new("tasks/a/todo.txt").unwrap();
    let refused = crate::lan_apply::Landed {
        refused: Some((file, "mkdir: Not a directory".to_owned())),
        ..crate::lan_apply::Landed::default()
    };
    let workspace = txtodo_store::WorkspaceId::new(Ulid::from_u128(9));
    ws.stuck_sync().book(device(1), workspace, &refused, 2_000);
    ws.stuck_sync().book(device(1), workspace, &refused, 3_000);
    for _ in 0..crate::peer_keys::PARK_AFTER {
        ws.peer_keys()
            .note_failure(Some(device(2)), crate::peer_keys::WRONG_GROUP);
    }
    let svc = TxtodoService::new(Arc::new(RwLock::new(ws)));

    let resp = svc
        .sync_status_impl(Request::new(pb::SyncStatusRequest { workspace: None }))
        .await
        .unwrap()
        .into_inner();

    let by_id = |n: u128| {
        let id = device(n).ulid().to_string();
        resp.peers.iter().find(|p| p.device == id).unwrap().clone()
    };
    let (one, two) = (by_id(1), by_id(2));
    assert_eq!(one.stuck.len(), 1);
    let s = &one.stuck[0];
    assert_eq!(s.workspace_id, workspace.to_string());
    assert_eq!(s.file, "tasks/a/todo.txt");
    assert_eq!(s.reason, "mkdir: Not a directory");
    assert_eq!((s.since_ms, s.last_ms, s.refusals), (2_000, 3_000, 2));
    assert!(!one.parked);
    assert!(two.stuck.is_empty());
    assert!(
        two.parked,
        "parked after {} wrong_group opens",
        crate::peer_keys::PARK_AFTER
    );
}
