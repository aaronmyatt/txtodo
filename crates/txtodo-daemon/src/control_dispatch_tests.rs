//! `drive_shared_session` as `control_dispatch.rs`'s sync-ALPN branch actually calls it: proven
//! over a real `ChannelLink` — same "one side real, one side scripted" harness
//! `lan_session_tests.rs` established, since a real `iroh` connection cannot be exercised
//! same-process in this sandbox. Stage 2 (task `daemon-workspace-session-multiplex`) replaced the
//! old peek-the-first-frame-and-route-to-one-workspace dance with this: the accept side already
//! knows every workspace it has open (`WorkspaceRoutes::list()`), so it hands them *all* to one
//! shared driver instead of picking one before the connection even starts.

use std::sync::Arc;

use crate::device_relay::{WorkspaceRoute, WorkspaceRoutes};
use crate::lan_session_dispatch::drive_shared_session;
use crate::lan_session_shared::LINK_WORKSPACE;
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
        drive_shared_session(&mut b_link, &routes, device_b, group);
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
async fn no_routes_at_all_is_dropped_without_panicking_or_hanging() {
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

    // No route registered at all — an empty routing table returns immediately without ever
    // reading the frame the peer sent, so it neither panics nor hangs.
    let routes = WorkspaceRoutes::new();
    tokio::task::spawn_blocking(move || {
        drive_shared_session(&mut b_link, &routes, peer_device(), group);
    })
    .await
    .unwrap_or_else(|e| panic!("driver task panicked: {e}"));
}

fn send_sealed(
    link: &mut dyn Link,
    group: txtodo_sync::GroupId,
    key: &GroupKey,
    msg: Message,
    workspace: txtodo_store::WorkspaceId,
) {
    let plain = msg.encode().unwrap_or_else(|e| panic!("encode: {e}"));
    let for_ = SealFor {
        group,
        epoch: GROUP_EPOCH,
        workspace,
    };
    let sealed =
        seal(plain.version, for_, key, &plain.body).unwrap_or_else(|e| panic!("seal: {e}"));
    link.send(Frame {
        version: plain.version,
        body: sealed,
    })
    .unwrap_or_else(|e| panic!("send: {e}"));
}

/// Sends a `Greet` for a workspace `peer_link`'s counterpart never registered — right alongside
/// the real handshake, on the same connection — then runs the real peer script for the workspace
/// that *is* registered, so the test can assert the unknown one never blocked convergence.
fn run_peer_with_unknown_greet_first(
    mut peer_link: txtodo_sync::ChannelLink,
    crypto: PeerCrypto,
    op: txtodo_model::Op,
    range: OriginRange,
) {
    let unknown_workspace = txtodo_store::WorkspaceId::new(Ulid::from_u128(0xBEEF));
    send_sealed(
        &mut peer_link,
        crypto.group,
        &crypto.key,
        Message::Greet {
            workspace: unknown_workspace.ulid().to_u128(),
            heads: Default::default(),
        },
        unknown_workspace,
    );
    run_peer_script(peer_link, crypto, op, range);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_message_for_a_workspace_this_side_never_opened_is_skipped_not_fatal() {
    // The module doc's "failure scope" claim, proven directly: a peer's `Greet` for a workspace
    // this side has no route for must not end the shared connection — the *known* workspace must
    // still converge afterward.
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
        drive_shared_session(&mut b_link, &routes, device_b, group);
    });
    let crypto = PeerCrypto {
        group,
        workspace,
        key,
        keys: keys.clone(),
    };
    let peer = tokio::task::spawn_blocking(move || {
        run_peer_with_unknown_greet_first(peer_link, crypto, op, range);
    });

    peer.await
        .unwrap_or_else(|e| panic!("peer task panicked: {e}"));
    driver
        .await
        .unwrap_or_else(|e| panic!("driver task panicked: {e}"));

    assert_todo_txt_has(&ws, "buy milk").await;
}

/// Sanity: [`LINK_WORKSPACE`] is imported and really is the sentinel `drive_shared_session` seals
/// its link-level `Hello` under — a regression guard against ever changing it to something a real
/// `WorkspaceId` could collide with.
#[test]
fn link_workspace_is_the_all_zero_sentinel() {
    assert_eq!(LINK_WORKSPACE.ulid().to_u128(), 0);
}
