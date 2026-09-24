//! `drive_control_session` end to end over a real `ChannelLink` (no network, but a real
//! `DeviceIdentity`, real `WorkspaceRegistry`, and real `seal_control`/`open_control`) — the same
//! "one side is the real driver under test, the other is scripted by hand" shape
//! `lan_session_tests.rs` uses, and for the same reason: running the *real* session on both ends
//! of a `ChannelLink` deadlocks, since (unlike a real `IrohLink`) it has no idle timeout to end an
//! exchange neither side explicitly closes.

use std::path::Path;
use std::sync::Mutex;

use txtodo_model::Ulid;
use txtodo_sync::{
    ControlMessage, GroupId, GroupKey, GroupKeys, KeyId, Link, Secret, channel_link_pair,
    open_control, seal_control,
};

use crate::clock::FakeClock;
use crate::control_session::drive_control_session;
use crate::device_identity::DeviceIdentity;
use crate::workspace_registry::WorkspaceRegistry;

const GROUP_EPOCH: u32 = 0;

fn make_identity(dir: &Path, group: GroupId, key: [u8; 32]) -> DeviceIdentity {
    let clock = FakeClock::new(1_000);
    let identity = DeviceIdentity::open_in_memory(dir, &clock).unwrap_or_else(|e| panic!("{e}"));
    identity.set_group(group);
    identity
        .key_store()
        .put(KeyId::Group(GROUP_EPOCH), &Secret::new(key.to_vec()))
        .unwrap_or_else(|e| panic!("seed group key: {e}"));
    identity
}

fn recv_plain(link: &mut dyn Link, group: GroupId, keys: &GroupKeys) -> ControlMessage {
    let frame = link.recv().unwrap_or_else(|e| panic!("recv: {e}"));
    open_control(&frame, group, keys).unwrap_or_else(|e| panic!("open: {e}"))
}

fn send_plain(link: &mut dyn Link, group: GroupId, key: &GroupKey, msg: ControlMessage) {
    let frame = seal_control(&msg, group, GROUP_EPOCH, key).unwrap_or_else(|e| panic!("seal: {e}"));
    link.send(frame).unwrap_or_else(|e| panic!("send: {e}"));
}

/// The full round trip: device B's real `drive_control_session` sends its own active workspace as
/// an `Offer` first (received and asserted by the test script, which plays device A's side of the
/// connection), then receives device A's own `Offer` and records it — proving both halves of the
/// exchange for real, not just one in isolation.
#[test]
fn drive_control_session_sends_and_records_offers_for_real() {
    let group = GroupId(42);
    let key = [7u8; 32];
    let dir_a = tempfile::tempdir().unwrap();
    let dir_b = tempfile::tempdir().unwrap();
    let identity_a = make_identity(dir_a.path(), group, key);
    let identity_b = make_identity(dir_b.path(), group, key);
    let device_a = identity_a.device();
    let device_b = identity_b.device();

    let clock = FakeClock::new(1_000);
    let registry_b_dir = tempfile::tempdir().unwrap();
    let workspace_b_dir = tempfile::tempdir().unwrap();
    let mut registry_b = WorkspaceRegistry::open(&registry_b_dir.path().join("registry.db"))
        .unwrap_or_else(|e| panic!("{e}"));
    let workspace_b_id = registry_b
        .add(workspace_b_dir.path(), &clock)
        .unwrap_or_else(|e| panic!("{e}"));
    let registry_b = Mutex::new(registry_b);

    let (mut link_a, mut link_b) = channel_link_pair();

    let session = std::thread::spawn(move || {
        drive_control_session(&mut link_b, &identity_b, &registry_b);
        identity_b
    });

    let group_key = GroupKey::from_bytes(key);
    let mut keys = GroupKeys::new();
    keys.insert(GROUP_EPOCH, group_key.clone()).unwrap();

    // B sends first (send_all_offers runs before its receive loop) — its one registered workspace.
    let from_b = recv_plain(&mut link_a, group, &keys);
    match from_b {
        ControlMessage::Offer {
            sender,
            workspace_id,
            ..
        } => {
            assert_eq!(sender, device_b);
            assert_eq!(workspace_id, workspace_b_id.ulid().to_u128());
        }
        other => panic!("expected an Offer from device B, got {other:?}"),
    }

    // Now A offers its own workspace back, then closes — ending B's receive loop, the same way a
    // real IrohLink's idle timeout eventually does in production.
    let offered_workspace = Ulid::from_u128(999).to_u128();
    send_plain(
        &mut link_a,
        group,
        &group_key,
        ControlMessage::Offer {
            sender: device_a,
            workspace_id: offered_workspace,
            name: "from-a".to_string(),
            offered_at_ms: 5_000,
        },
    );
    drop(link_a);

    let identity_b = session
        .join()
        .unwrap_or_else(|_| panic!("session thread panicked"));
    let recorded = identity_b.workspace_offers().list();
    assert_eq!(recorded.len(), 1);
    assert_eq!(recorded[0].offering_device, device_a);
    assert_eq!(recorded[0].workspace_id.ulid().to_u128(), offered_workspace);
    assert_eq!(recorded[0].name, "from-a");
}

/// Task `control-channel-keystore-visibility`: a group-key read that fails leaves its reason on the
/// device's `LanStatus` (what `Health` and `WorkspacePendingOffers` report), and the next session
/// that reads the key cleanly clears it. A corrupt stored key stands in for the OS keystore not
/// answering: both take the same path.
#[test]
fn a_failed_group_key_read_is_recorded_and_a_good_one_clears_it() {
    let dir = tempfile::tempdir().unwrap();
    let identity = DeviceIdentity::open_in_memory(dir.path(), &FakeClock::new(1_000)).unwrap();
    identity
        .key_store()
        .put(KeyId::Group(GROUP_EPOCH), &Secret::new(vec![1, 2, 3]))
        .unwrap();
    let registry_dir = tempfile::tempdir().unwrap();
    let registry =
        Mutex::new(WorkspaceRegistry::open(&registry_dir.path().join("registry.db")).unwrap());

    let (_peer, mut link) = channel_link_pair();
    drive_control_session(&mut link, &identity, &registry);
    let (why, _at_ms) = identity
        .lan_status()
        .offers_problem()
        .unwrap_or_else(|| panic!("the failure is recorded"));
    assert!(why.contains("3 bytes"), "{why}");

    identity
        .key_store()
        .put(KeyId::Group(GROUP_EPOCH), &Secret::new(vec![7; 32]))
        .unwrap();
    let (peer, mut link) = channel_link_pair();
    drop(peer);
    drive_control_session(&mut link, &identity, &registry);
    assert!(identity.lan_status().offers_problem().is_none(), "cleared");
}
