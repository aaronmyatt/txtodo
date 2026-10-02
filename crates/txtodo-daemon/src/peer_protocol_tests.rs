//! Task sync-divergence-check/protocol-mismatch: a peer on another sync protocol is named, not
//! just dropped. A frame of another version (what a v3 peer sends a v2 daemon, and back), or a
//! link `Hello` with another `protocol`, ends a sync session as `OtherProtocol`; a control session
//! reports it too; `PeerKeys` keeps it per known peer until a session greets, and never parks it.

use std::collections::BTreeMap;
use std::sync::Mutex;
use std::time::Duration;

use txtodo_model::{DeviceId, Ulid};
use txtodo_sync::{
    Frame, FrameError, GroupKey, Link, LinkError, Message, PROTOCOL_VERSION, SealFor,
    channel_link_pair, seal,
};

use crate::control_session::drive_control_session;
use crate::lan_session_shared::LINK_WORKSPACE;
use crate::lan_session_tests::{drive_session_end, make_workspace, peer_device};
use crate::live_peers::Carrier;
use crate::peer_keys::{PeerKeys, PeerSignal, SessionEnd};
use crate::peer_keys_dial_tests::identity_in;
use crate::workspace_registry::WorkspaceRegistry;

const THEIRS: u16 = PROTOCOL_VERSION + 1;

/// What a link to a peer on another protocol does: our frames go out, each of theirs fails to
/// decode with `UnknownVersion` (`Frame::decode`, `lan_link.rs`).
struct OtherVersionLink;

impl Link for OtherVersionLink {
    fn send(&mut self, _frame: Frame) -> Result<(), LinkError> {
        Ok(())
    }

    fn recv(&mut self) -> Result<Frame, LinkError> {
        Err(LinkError::Frame(FrameError::UnknownVersion {
            got: THEIRS,
            supported: PROTOCOL_VERSION,
        }))
    }

    fn recv_timeout(&mut self, _wait: Duration) -> Result<Option<Frame>, LinkError> {
        self.recv().map(Some)
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_frame_of_another_version_ends_the_session_naming_it() {
    let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
    let (ws, device, group, _) = make_workspace(dir.path(), [7; 32]);
    let end = tokio::task::spawn_blocking(move || {
        drive_session_end(&mut OtherVersionLink, (ws, device, group), Carrier::Lan)
    })
    .await
    .unwrap_or_else(|e| panic!("driver: {e}"));
    assert_eq!(end, SessionEnd::OtherProtocol(THEIRS));
    assert!(!end.greeted());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_hello_with_another_protocol_ends_the_session_naming_it() {
    let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
    let (ws, device, group, _) = make_workspace(dir.path(), [7; 32]);
    let (mut peer_link, mut b_link) = channel_link_pair();
    let driver = tokio::task::spawn_blocking(move || {
        drive_session_end(&mut b_link, (ws, device, group), Carrier::Lan)
    });
    assert!(peer_link.recv().is_ok(), "our own Hello goes out first");
    let plain = Message::Hello {
        device: peer_device(),
        group,
        heads: BTreeMap::new(),
        protocol: THEIRS,
        wall_ms: 1_000,
    }
    .encode()
    .unwrap_or_else(|e| panic!("encode: {e}"));
    let for_ = SealFor {
        group,
        epoch: 0,
        workspace: LINK_WORKSPACE,
    };
    let key = GroupKey::from_bytes([7; 32]);
    let body = seal(plain.version, for_, &key, &plain.body).unwrap_or_else(|e| panic!("{e}"));
    let sent = peer_link.send(Frame {
        version: plain.version,
        body,
    });
    assert!(sent.is_ok());
    let end = driver.await.unwrap_or_else(|e| panic!("driver: {e}"));
    assert_eq!(end, SessionEnd::OtherProtocol(THEIRS));
}

#[test]
fn a_control_frame_of_another_version_is_reported() {
    let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
    let identity = identity_in(dir.path(), [7; 32]);
    let registry = Mutex::new(
        WorkspaceRegistry::open(&dir.path().join("registry.db"))
            .unwrap_or_else(|e| panic!("registry: {e}")),
    );
    let seen = drive_control_session(&mut OtherVersionLink, &identity, &registry);
    assert_eq!(seen, PeerSignal::OtherProtocol(THEIRS));
}

fn peer(n: u128) -> DeviceId {
    DeviceId::new(Ulid::from_u128(n))
}

#[test]
fn a_known_peer_keeps_its_protocol_until_a_session_greets_and_is_never_parked() {
    let keys = PeerKeys::default();
    assert_eq!(keys.other_protocol(peer(1)), None);
    for _ in 0..5 {
        keys.book_session(Some(peer(1)), SessionEnd::OtherProtocol(THEIRS));
    }
    assert_eq!(keys.other_protocol(peer(1)), Some(THEIRS));
    assert!(
        !keys.is_parked(peer(1)),
        "dialing it is how we learn it was upgraded"
    );
    keys.book_session(Some(peer(1)), SessionEnd::NoHello);
    assert_eq!(
        keys.other_protocol(peer(1)),
        Some(THEIRS),
        "silence says nothing"
    );
    keys.book_session(Some(peer(1)), SessionEnd::Greeted(peer(1)));
    assert_eq!(
        keys.other_protocol(peer(1)),
        None,
        "upgraded: it greets again"
    );

    keys.book(None, PeerSignal::OtherProtocol(THEIRS), "sync");
    assert_eq!(
        keys.other_protocol(peer(2)),
        None,
        "an unknown peer names nobody"
    );
}
