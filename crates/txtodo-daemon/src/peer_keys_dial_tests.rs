//! Task sync-drift line 5, wired end to end: a sync or control session with a peer of another
//! group ends naming `wrong_group`; a sync session whose peer never sends its `Hello` is not a
//! successful dial; the dial side then backs off, parks the peer and drops it from every dial set,
//! and a pairing or an in-group sighting brings it back. The LAN fallback dials a peer's relay
//! node id, never its LAN one.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use txtodo_model::{DeviceId, Ulid};
use txtodo_store::NewDevice;
use txtodo_sync::{
    Announcement, ControlMessage, DiscoveredPeer, Frame, GroupId, GroupKey, KeyId, Link, Message,
    PROTOCOL_VERSION, SealFor, Secret, Sighting, backoff_ms, channel_link_pair, seal, seal_control,
};

use crate::clock::FakeClock;
use crate::control_session::drive_control_session;
use crate::device_identity::DeviceIdentity;
use crate::lan_peers::{
    DialState, KnownPeers, SharedDialState, book_dial, peers_to_resync, remember_any_sighting,
    try_begin_dial,
};
use crate::lan_session_shared::LINK_WORKSPACE;
use crate::lan_session_tests::{drive_session_end, make_workspace, peer_device};
use crate::live_peers::Carrier;
use crate::peer_keys::{PARK_AFTER, PeerSignal, SessionEnd, WRONG_GROUP};
use crate::relay_fallback::peer_relay_node;
use crate::workspace_registry::WorkspaceRegistry;

/// A link `Hello` from [`peer_device`], sealed under `group` with `key`.
fn hello_frame(group: GroupId, key: &GroupKey) -> Frame {
    let plain = Message::Hello {
        device: peer_device(),
        group,
        heads: BTreeMap::new(),
        protocol: PROTOCOL_VERSION,
        wall_ms: 1_000,
    }
    .encode()
    .unwrap_or_else(|e| panic!("encode: {e}"));
    let for_ = SealFor {
        group,
        epoch: 0,
        workspace: LINK_WORKSPACE,
    };
    let body = seal(plain.version, for_, key, &plain.body).unwrap_or_else(|e| panic!("seal: {e}"));
    Frame {
        version: plain.version,
        body,
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_hello_sealed_for_another_group_ends_the_session_refused() {
    let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
    let (ws, device, group, _) = make_workspace(dir.path(), [7; 32]);
    let (mut peer_link, mut b_link) = channel_link_pair();
    let driver = tokio::task::spawn_blocking(move || {
        drive_session_end(&mut b_link, (ws, device, group), Carrier::Lan)
    });
    let ours = peer_link.recv();
    assert!(ours.is_ok(), "our own Hello goes out first");
    let other = GroupId(group.0 ^ 1);
    let sent = peer_link.send(hello_frame(other, &GroupKey::from_bytes([9; 32])));
    assert!(sent.is_ok());
    let end = driver.await.unwrap_or_else(|e| panic!("driver: {e}"));
    assert_eq!(end, SessionEnd::Refused(WRONG_GROUP));
    assert!(!end.greeted());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_session_whose_peer_never_sends_its_hello_is_not_greeted() {
    let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
    let (ws, device, group, _) = make_workspace(dir.path(), [7; 32]);
    let (mut peer_link, mut b_link) = channel_link_pair();
    let driver = tokio::task::spawn_blocking(move || {
        drive_session_end(&mut b_link, (ws, device, group), Carrier::Lan)
    });
    assert!(peer_link.recv().is_ok(), "our own Hello went out");
    drop(peer_link);
    let end = driver.await.unwrap_or_else(|e| panic!("driver: {e}"));
    assert_eq!(end, SessionEnd::NoHello, "sending our Hello is no success");
    assert!(!end.greeted());
}

fn identity_in(dir: &std::path::Path, key: [u8; 32]) -> DeviceIdentity {
    let identity = DeviceIdentity::open_in_memory(dir, &FakeClock::new(1_000))
        .unwrap_or_else(|e| panic!("identity: {e}"));
    identity
        .key_store()
        .put(KeyId::Group(0), &Secret::new(key.to_vec()))
        .unwrap_or_else(|e| panic!("seed group key: {e}"));
    identity
}

#[test]
fn a_control_frame_sealed_for_another_group_is_reported() {
    let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
    let identity = identity_in(dir.path(), [7; 32]);
    let registry_path = dir.path().join("registry.db");
    let registry = Mutex::new(
        WorkspaceRegistry::open(&registry_path).unwrap_or_else(|e| panic!("registry: {e}")),
    );
    let (mut peer_link, mut link) = channel_link_pair();
    let offer = ControlMessage::Offer {
        sender: peer_device(),
        workspace_id: 1,
        name: String::new(),
        offered_at_ms: 1_000,
    };
    let other = GroupId(identity.group().0 ^ 1);
    let frame = seal_control(&offer, other, 0, &GroupKey::from_bytes([9; 32]))
        .unwrap_or_else(|e| panic!("seal: {e}"));
    assert!(peer_link.send(frame).is_ok());
    let seen = drive_control_session(&mut link, &identity, &registry);
    assert_eq!(seen, PeerSignal::OpenFailed(WRONG_GROUP));
}

fn high_peer(n: u128) -> DeviceId {
    DeviceId::new(Ulid::from_u128(u128::MAX - n))
}

fn known(peers: &[DeviceId]) -> KnownPeers {
    let map = peers
        .iter()
        .map(|&device| {
            let peer = DiscoveredPeer {
                device,
                node: [3; 32],
                addresses: Vec::new(),
            };
            (device, peer)
        })
        .collect();
    Arc::new(Mutex::new(map))
}

fn resync_set(known: &KnownPeers, identity: &DeviceIdentity) -> Vec<DeviceId> {
    peers_to_resync(known, identity)
        .into_iter()
        .map(|p| p.device)
        .collect()
}

#[test]
fn a_wrong_group_peer_backs_off_is_parked_and_dropped_from_the_resync_set() {
    let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
    let identity = identity_in(dir.path(), [7; 32]);
    let (stale, fine) = (high_peer(1), high_peer(2));
    assert!(identity.device() < fine, "we are the dialing side");
    let known = known(&[stale, fine]);
    let dial_state: SharedDialState = Arc::new(Mutex::new(DialState::default()));
    let mut now = 0;
    for _ in 0..PARK_AFTER {
        assert!(try_begin_dial(&dial_state, stale, now));
        book_dial(
            &identity,
            &dial_state,
            stale,
            SessionEnd::Refused(WRONG_GROUP),
        );
        now += backoff_ms(PARK_AFTER);
    }
    assert!(
        !try_begin_dial(
            &dial_state,
            stale,
            now - backoff_ms(PARK_AFTER) + backoff_ms(0)
        ),
        "the backoff kept growing: a refused session is a failed dial"
    );
    assert!(identity.peer_keys().is_parked(stale));
    assert_eq!(resync_set(&known, &identity), vec![fine]);
}

#[test]
fn a_parked_peer_comes_back_on_an_in_group_sighting() {
    let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
    let identity = identity_in(dir.path(), [7; 32]);
    let stale = high_peer(1);
    let known = known(&[stale]);
    for _ in 0..PARK_AFTER {
        identity
            .peer_keys()
            .book_session(Some(stale), SessionEnd::Refused(WRONG_GROUP));
    }
    let sighting = |group: GroupId| Sighting {
        announcement: Announcement {
            device: stale,
            group,
            proto: PROTOCOL_VERSION,
            node: [3; 32],
        },
        addresses: Vec::new(),
    };
    remember_any_sighting(&identity, &sighting(GroupId(identity.group().0 ^ 1)));
    assert!(
        resync_set(&known, &identity).is_empty(),
        "still another group"
    );
    remember_any_sighting(&identity, &sighting(identity.group()));
    assert_eq!(resync_set(&known, &identity), vec![stale]);
}

fn register(identity: &DeviceIdentity, device: DeviceId) {
    let new = NewDevice {
        device,
        name: String::new(),
        static_public: [0; txtodo_store::DEVICE_STATIC_KEY_BYTES],
        paired_at_ms: 1,
        last_known_wall_ms: None,
        key_epoch: 0,
    };
    let mut store = identity
        .store()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    store
        .register_device_as(&new, true)
        .unwrap_or_else(|e| panic!("register: {e}"));
}

#[test]
fn the_lan_fallback_dials_the_recorded_relay_node_id_or_nothing() {
    let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
    let identity = identity_in(dir.path(), [7; 32]);
    let (with, without, removed) = (high_peer(1), high_peer(2), high_peer(3));
    for device in [with, without, removed] {
        register(&identity, device);
    }
    {
        let mut store = identity
            .store()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for device in [with, removed] {
            store
                .set_relay_reachability(device, [5; 32], "relay:example")
                .unwrap_or_else(|e| panic!("reachability: {e}"));
        }
        store
            .remove_device(removed, 2)
            .unwrap_or_else(|e| panic!("remove: {e}"));
    }
    assert_eq!(peer_relay_node(&identity, with), Some([5; 32]));
    assert_eq!(peer_relay_node(&identity, without), None, "none recorded");
    assert_eq!(peer_relay_node(&identity, removed), None, "removed");
    assert_eq!(peer_relay_node(&identity, high_peer(4)), None, "unknown");
}
