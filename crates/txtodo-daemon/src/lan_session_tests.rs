//! `drive_session` end to end over a real `ChannelLink` (no sockets, but real `Session`, real
//! `seal`/`open`, and a real `FileActor` commit): the only place in this codebase that can prove
//! the LAN sync engine's protocol driving is correct, since a real `iroh` connection cannot be
//! exercised same-host in this sandbox (`lan.rs`'s module doc, `txtodo-sync`'s `CLAUDE.md`). One
//! side is the real `drive_session` under test; the other is scripted by hand in this test, since
//! there is deliberately no second production `Session` role for "serve a Want" (`lan_session.rs`'s
//! own module doc: the sending side is stateless, not a second `Session`).

use std::collections::BTreeMap;
use std::sync::{Arc, RwLock};

use crate::clock::FakeClock;
use crate::lan_session::drive_session;
use crate::server::SharedWorkspace;
use crate::workspace::Workspace;
use txtodo_model::{
    DeviceId, FilePath, Hlc, IdentityMode, Op, OpId, OpKind, Principal, TaskId, Ulid,
};
use txtodo_store::WorkspaceId;
use txtodo_sync::{
    Frame, GroupId, GroupKey, GroupKeys, KeyId, Link, Message, OriginRange, PROTOCOL_VERSION,
    SealFor, Secret, channel_link_pair, open, seal,
};

const GROUP_EPOCH: u32 = 0;

/// `pub(crate)`: reused by `lan_session_security_tests` (`security-m4-review`'s "no secrets in
/// logs" check), which needs the exact same scripted peer identity.
pub(crate) fn peer_device() -> DeviceId {
    DeviceId::new(Ulid::from_u128(9_999))
}

/// A fresh, real `Workspace` seeded with a group key so `drive_session` has something to seal
/// with — everything else (device id, group id) is whatever the workspace mints on its own.
/// `pub(crate)`: reused by `relay_fallback_tests` (same reasons as [`peer_device`]'s doc).
pub(crate) fn make_workspace(
    dir: &std::path::Path,
    key: [u8; 32],
) -> (SharedWorkspace, DeviceId, GroupId, WorkspaceId) {
    let clock: Arc<dyn crate::clock::Clock> = Arc::new(FakeClock::new(1_000));
    let ws = Workspace::open_with_default_mode(dir, clock, IdentityMode::Tagged)
        .unwrap_or_else(|e| panic!("open workspace: {e}"));
    ws.key_store()
        .put(KeyId::Group(GROUP_EPOCH), &Secret::new(key.to_vec()))
        .unwrap_or_else(|e| panic!("seed group key: {e}"));
    let device = ws.device();
    let group = ws.group();
    let workspace = ws.workspace_id();
    (Arc::new(RwLock::new(ws)), device, group, workspace)
}

fn send(link: &mut dyn Link, group: GroupId, workspace: WorkspaceId, key: &GroupKey, msg: Message) {
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

/// Peeks the frame's own workspace rather than being told one — stage 2: an incoming frame may be
/// the link-level `Hello` (sealed under `LINK_WORKSPACE`) or a real workspace's own message, and a
/// real caller (`lan_session_dispatch.rs`) never knows which up front either.
fn recv(link: &mut dyn Link, group: GroupId, keys: &GroupKeys) -> Message {
    let frame = link.recv().unwrap_or_else(|e| panic!("recv: {e}"));
    let workspace = txtodo_sync::peek_workspace(&frame.body)
        .unwrap_or_else(|| panic!("frame too short to peek a workspace"));
    let plain = open(frame.version, group, workspace, keys, &frame.body)
        .unwrap_or_else(|e| panic!("open: {e}"));
    Message::decode(&Frame {
        version: frame.version,
        body: plain,
    })
    .unwrap_or_else(|e| panic!("decode: {e}"))
}

/// `pub(crate)`: see [`peer_device`]'s doc for why.
pub(crate) fn one_peer_op(task: TaskId) -> Op {
    Op {
        id: OpId::new(Ulid::from_u128(2)),
        hlc: Hlc {
            wall_ms: 1_000,
            counter: 1,
            device: peer_device(),
        },
        principal: Principal::User {
            device: peer_device(),
        },
        file: FilePath::new("todo.txt").unwrap_or_else(|e| panic!("{e}")),
        kind: OpKind::Insert {
            task,
            after: None,
            line: format!("buy milk id:{task}"),
        },
    }
}

/// The group crypto context both sides of the test need — bundled so `run_peer_script` stays
/// under `maxParams`. `pub(crate)`: see [`peer_device`]'s doc for why.
pub(crate) struct PeerCrypto {
    pub(crate) group: GroupId,
    pub(crate) workspace: WorkspaceId,
    pub(crate) key: GroupKey,
    pub(crate) keys: GroupKeys,
}

/// The stage-2 handshake half of [`run_peer_script`]: our own link-level `Hello` (no heads —
/// those moved to `Greet`), sealed under the reserved sentinel, then our own `Greet` for the one
/// workspace this test cares about; then asserts the driven side answers in the same order (its
/// own `Hello`, its own `Greet`, then — once it has processed ours — the `Want` that workspace
/// derived). Split out of `run_peer_script` purely to keep that function under this workspace's
/// line budget (`clippy.toml`).
fn peer_handshake(peer_link: &mut txtodo_sync::ChannelLink, crypto: &PeerCrypto, range: OriginRange) {
    let PeerCrypto {
        group,
        workspace,
        key,
        keys,
    } = crypto;
    let (group, workspace) = (*group, *workspace);
    send(
        peer_link,
        group,
        crate::lan_session_shared::LINK_WORKSPACE,
        key,
        Message::Hello {
            device: peer_device(),
            group,
            heads: BTreeMap::new(),
            protocol: PROTOCOL_VERSION,
            wall_ms: 1_000,
        },
    );
    let mut heads = BTreeMap::new();
    heads.insert(peer_device(), 1u64);
    send(
        peer_link,
        group,
        workspace,
        key,
        Message::Greet {
            workspace: workspace.ulid().to_u128(),
            heads,
        },
    );
    assert!(matches!(recv(peer_link, group, keys), Message::Hello { .. }));
    assert!(matches!(recv(peer_link, group, keys), Message::Greet { .. }));
    let want = recv(peer_link, group, keys);
    assert_eq!(
        want,
        Message::Want {
            workspace: workspace.ulid().to_u128(),
            ranges: vec![range]
        }
    );
}

/// The scripted peer's whole side of the exchange: offer one op, serve it once wanted, check the
/// ack. Runs on its own blocking thread since `send`/`recv` block. `pub(crate)`: see
/// [`peer_device`]'s doc for why.
pub(crate) fn run_peer_script(
    mut peer_link: txtodo_sync::ChannelLink,
    crypto: PeerCrypto,
    op: Op,
    range: OriginRange,
) {
    peer_handshake(&mut peer_link, &crypto, range);
    let PeerCrypto {
        group,
        workspace,
        key,
        keys,
    } = crypto;
    let signing_key = txtodo_sync::derive_group_op_signing_key(&key);
    let signature = txtodo_sync::sign(&op, &signing_key).unwrap_or_else(|e| panic!("sign: {e}"));
    send(
        &mut peer_link,
        group,
        workspace,
        &key,
        Message::Ops {
            workspace: workspace.ulid().to_u128(),
            ops: vec![op],
            signatures: vec![signature],
            ranges: vec![range],
        },
    );
    let ack = recv(&mut peer_link, group, &keys);
    assert_eq!(
        ack,
        Message::Ack {
            workspace: workspace.ulid().to_u128(),
            committed: vec![range]
        }
    );
    // Dropping `peer_link` here closes the channel, so `drive_session`'s next `recv` on `b_link`
    // returns `LinkError::Closed` and it returns cleanly instead of blocking forever.
}

/// `pub(crate)`: reused by `relay_fallback_tests` (same reasons as [`peer_device`]'s doc).
pub(crate) async fn assert_todo_txt_has(ws: &SharedWorkspace, needle: &str) {
    let handle = {
        let guard = ws.read().unwrap_or_else(std::sync::PoisonError::into_inner);
        guard
            .actor(&FilePath::new("todo.txt").unwrap_or_else(|e| panic!("{e}")))
            .cloned()
    };
    let bytes = handle
        .expect("todo.txt was registered on first sync write")
        .get()
        .await
        .unwrap_or_else(|e| panic!("{e}"))
        .bytes;
    let text = String::from_utf8_lossy(&bytes);
    assert!(text.contains(needle), "{text}");
}

#[tokio::test(flavor = "multi_thread")]
async fn drive_session_pulls_a_peers_op_and_acks_what_it_committed() {
    let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
    let key_bytes = [7u8; 32];
    let key = GroupKey::from_bytes(key_bytes);
    let (ws, device_b, group, workspace) = make_workspace(dir.path(), key_bytes);
    let mut keys = GroupKeys::new();
    keys.insert(GROUP_EPOCH, key.clone())
        .unwrap_or_else(|e| panic!("{e:?}"));

    let (peer_link, mut b_link) = channel_link_pair();
    let task = TaskId::new(Ulid::from_u128(1));
    let op = one_peer_op(task);
    let range = OriginRange {
        device: peer_device(),
        first: 1,
        last: 1,
    };

    let ws_for_driver = Arc::clone(&ws);
    let driver = tokio::task::spawn_blocking(move || {
        drive_session(&mut b_link, ws_for_driver, device_b, group);
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
