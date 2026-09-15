//! `drive_routed_sync_connection`: the actual routing this task's stage 3 adds, proven over a real
//! `ChannelLink` — same "one side real, one side scripted" harness `lan_session_tests.rs`
//! established, since a real `iroh` connection cannot be exercised same-process in this sandbox.

use std::sync::Arc;

use crate::control_dispatch::drive_routed_sync_connection;
use crate::device_relay::{WorkspaceRoute, WorkspaceRoutes};
use crate::lan_session_tests::{
    PeerCrypto, assert_todo_txt_has, make_workspace, one_peer_op, peer_device, run_peer_script,
};
use txtodo_model::{TaskId, Ulid};
use txtodo_sync::{
    Frame, GroupKey, GroupKeys, Link, Message, OriginRange, PROTOCOL_VERSION, SealFor,
    channel_link_pair, seal,
};

const GROUP_EPOCH: u32 = 0;

#[tokio::test(flavor = "multi_thread")]
async fn a_registered_workspace_routes_to_the_real_session_and_converges() {
    let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
    let key_bytes = [7u8; 32];
    let key = GroupKey::from_bytes(key_bytes);
    let (ws, device_b, group, workspace) = make_workspace(dir.path(), key_bytes);
    let mut keys = GroupKeys::new();
    keys.insert(GROUP_EPOCH, key.clone())
        .unwrap_or_else(|e| panic!("{e:?}"));

    let routes = WorkspaceRoutes::new();
    routes
        .register(
            workspace,
            WorkspaceRoute {
                ws: Arc::clone(&ws),
                device: device_b,
                group,
            },
        )
        .unwrap_or_else(|e| panic!("{e}"));

    let (peer_link, mut b_link) = channel_link_pair();
    let task = TaskId::new(Ulid::from_u128(1));
    let op = one_peer_op(task);
    let range = OriginRange {
        device: peer_device(),
        first: 1,
        last: 1,
    };

    let driver = tokio::task::spawn_blocking(move || {
        drive_routed_sync_connection(&mut b_link, &routes);
    });
    let crypto = PeerCrypto {
        group,
        workspace,
        key,
        keys: keys.clone(),
    };
    let peer = tokio::task::spawn_blocking(move || {
        run_peer_script(peer_link, crypto, op, range);
    });

    peer.await
        .unwrap_or_else(|e| panic!("peer task panicked: {e}"));
    driver
        .await
        .unwrap_or_else(|e| panic!("driver task panicked: {e}"));

    assert_todo_txt_has(&ws, "buy milk").await;
}

#[tokio::test(flavor = "multi_thread")]
async fn an_unrouted_workspace_is_dropped_without_panicking_or_hanging() {
    let group = txtodo_sync::GroupId(1);
    let workspace = txtodo_store::WorkspaceId::new(Ulid::from_u128(0xDEAD));
    let key = GroupKey::from_bytes([9u8; 32]);
    let hello = Message::Hello {
        device: peer_device(),
        group,
        heads: Default::default(),
        protocol: PROTOCOL_VERSION,
        wall_ms: 0,
    };
    let plain = hello.encode().unwrap_or_else(|e| panic!("encode: {e}"));
    let sealed = seal(
        plain.version,
        SealFor {
            group,
            epoch: GROUP_EPOCH,
            workspace,
        },
        &key,
        &plain.body,
    )
    .unwrap_or_else(|e| panic!("seal: {e}"));

    let (mut peer_link, mut b_link) = channel_link_pair();
    peer_link
        .send(Frame {
            version: plain.version,
            body: sealed,
        })
        .unwrap_or_else(|e| panic!("send: {e}"));

    // No route registered for `workspace` at all — the empty table is the point of this test.
    let routes = WorkspaceRoutes::new();
    tokio::task::spawn_blocking(move || {
        drive_routed_sync_connection(&mut b_link, &routes);
    })
    .await
    .unwrap_or_else(|e| panic!("driver task panicked: {e}"));
}
