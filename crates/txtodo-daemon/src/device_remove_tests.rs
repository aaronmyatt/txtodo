//! `Workspace::remove_device` (plan M4 tasks/sync-device-remove): guards (self, last device,
//! idempotent) and the happy path (rotates the epoch, tombstones the row, advances the remaining
//! devices' `key_epoch`). Peers are registered directly on the store rather than through a real
//! pairing handshake (`pairing_grpc_tests.rs` already covers that registration path) so these tests
//! stay focused on removal/rotation alone.

use std::sync::Arc;

use txtodo_model::{DeviceId, Ulid};
use txtodo_store::NewDevice;
use txtodo_sync::KeyId;

use crate::clock::{Clock, FakeClock};
use crate::device_remove::RemoveDeviceError;
use crate::workspace::Workspace;
use txtodo_sync::RemovalError;

fn workspace() -> Workspace {
    let dir = tempfile::tempdir().unwrap();
    // Leak the tempdir so it outlives the Workspace; these tests never touch the filesystem again.
    let dir = Box::leak(Box::new(dir));
    Workspace::open(
        dir.path(),
        Arc::new(FakeClock::new(1_000)) as Arc<dyn Clock>,
    )
    .unwrap_or_else(|e| panic!("open: {e}"))
}

fn device(n: u128) -> DeviceId {
    DeviceId::new(Ulid::from_u128(n))
}

fn register_peer(ws: &Workspace, n: u128) {
    let mut store = ws.store().lock().unwrap();
    store
        .register_device(&NewDevice {
            device: device(n),
            name: format!("peer-{n}"),
            static_public: [n as u8; 32],
            paired_at_ms: 1_000,
            last_known_wall_ms: None,
            key_epoch: 0,
        })
        .unwrap();
}

#[test]
fn removing_self_is_refused() {
    let ws = workspace();
    register_peer(&ws, 1);
    let err = ws.remove_device(ws.device(), 2_000).unwrap_err();
    assert!(matches!(
        err,
        RemoveDeviceError::Removal(RemovalError::CannotRemoveSelf { .. })
    ));
}

#[test]
fn removing_the_last_device_in_a_solo_group_is_refused() {
    let ws = workspace();
    // No peers registered: this device is the whole group.
    let err = ws.remove_device(device(404), 2_000).unwrap_err();
    assert!(matches!(
        err,
        RemoveDeviceError::Removal(RemovalError::CannotRemoveLastDevice)
    ));
}

#[test]
fn removing_an_unknown_device_reports_removed_false_and_rotates_nothing() {
    let ws = workspace();
    register_peer(&ws, 1); // devices_before = 2 (self + peer 1), so validate_removal passes
    let outcome = ws.remove_device(device(404), 2_000).unwrap();
    assert!(!outcome.removed);
    assert!(!outcome.already_removed);
    assert_eq!(outcome.rotated_to_epoch, 0);
    assert_eq!(ws.group_epoch(), 0, "nothing rotated");
}

#[test]
fn removing_a_real_peer_rotates_the_epoch_and_tombstones_the_row() {
    let ws = workspace();
    register_peer(&ws, 1);
    register_peer(&ws, 2);

    let outcome = ws.remove_device(device(1), 2_000).unwrap();
    assert!(outcome.removed);
    assert!(!outcome.already_removed);
    assert_eq!(outcome.rotated_to_epoch, 1);
    assert_eq!(outcome.grants_minted, 1, "one grant for peer 2");
    assert_eq!(ws.group_epoch(), 1);

    let store = ws.store().lock().unwrap();
    let removed = store.device(device(1)).unwrap().unwrap();
    assert_eq!(removed.removed_at_ms, Some(2_000));
    let remaining = store.device(device(2)).unwrap().unwrap();
    assert!(remaining.removed_at_ms.is_none());
    assert_eq!(
        remaining.key_epoch, 1,
        "the remaining peer's epoch advanced"
    );
    drop(store);

    let new_key = ws.key_store().get(KeyId::Group(1)).unwrap();
    assert!(new_key.is_some(), "the new epoch's key is stored locally");
}

#[test]
fn removing_the_only_remaining_peer_still_rotates_with_zero_grants() {
    let ws = workspace();
    register_peer(&ws, 1);

    let outcome = ws.remove_device(device(1), 2_000).unwrap();
    assert!(outcome.removed);
    assert_eq!(outcome.rotated_to_epoch, 1);
    assert_eq!(
        outcome.grants_minted, 0,
        "nobody left to grant the new epoch to"
    );
    assert_eq!(ws.group_epoch(), 1);
}

#[test]
fn removing_an_already_removed_device_is_idempotent_and_rotates_nothing_again() {
    let ws = workspace();
    register_peer(&ws, 1);
    register_peer(&ws, 2);

    let first = ws.remove_device(device(1), 2_000).unwrap();
    assert_eq!(first.rotated_to_epoch, 1);

    let second = ws.remove_device(device(1), 3_000).unwrap();
    assert!(second.removed);
    assert!(second.already_removed);
    assert_eq!(
        second.rotated_to_epoch, 0,
        "nothing rotated the second time"
    );
    assert_eq!(
        ws.group_epoch(),
        1,
        "still at the epoch the first removal minted"
    );
}
