//! Known sync-group peers (plan M4 tasks/sync-device-remove): register → list → remove round-trips,
//! the schema lands at 6, and a removed row is kept (tombstoned) rather than deleted.
// Integration tests are tests: clippy.toml allows unwrap/expect in #[test] fns but not in their helpers.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::path::Path;
use txtodo_model::{DeviceId, Ulid};
use txtodo_store::{NewDevice, Store};

fn open(dir: &Path) -> Store {
    Store::open(&dir.join("oplog.db")).unwrap_or_else(|e| panic!("open: {e}"))
}

fn device(n: u128) -> DeviceId {
    DeviceId::new(Ulid::from_u128(n))
}

fn new_device(n: u128, key: u8, paired_at_ms: u64) -> NewDevice {
    NewDevice {
        device: device(n),
        name: format!("device-{n}"),
        static_public: [key; 32],
        paired_at_ms,
        last_known_wall_ms: None,
        key_epoch: 0,
    }
}

#[test]
fn migrating_to_devices_lands_the_schema_at_six() {
    let dir = tempfile::tempdir().unwrap();
    let store = open(dir.path());
    assert_eq!(store.user_version().unwrap(), 6);
}

#[test]
fn register_list_and_get_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = open(dir.path());
    store.register_device(&new_device(1, 0xAA, 1_000)).unwrap();
    store.register_device(&new_device(2, 0xBB, 2_000)).unwrap();

    let listed = store.list_devices().unwrap();
    assert_eq!(listed.len(), 2);
    assert_eq!(listed[0].device, device(1), "oldest paired_at first");
    assert_eq!(listed[0].static_public, [0xAAu8; 32]);
    assert_eq!(listed[0].key_epoch, 0);
    assert!(listed[0].removed_at_ms.is_none());
    assert!(listed[0].last_known_wall_ms.is_none());

    let one = store.device(device(1)).unwrap().unwrap();
    assert_eq!(one.name, "device-1");
    assert!(store.device(device(404)).unwrap().is_none());
}

#[test]
fn removing_a_device_tombstones_it_rather_than_deleting_it() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = open(dir.path());
    store.register_device(&new_device(1, 0xAA, 1_000)).unwrap();

    let existed = store.remove_device(device(1), 5_000).unwrap();
    assert!(existed, "the device existed");

    // Still listed — removed rows are kept, never deleted (the wire boundary decides visibility).
    let listed = store.list_devices().unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].removed_at_ms, Some(5_000));

    // Idempotent: removing twice just replaces removed_at and still reports true.
    let again = store.remove_device(device(1), 6_000).unwrap();
    assert!(again);
    assert_eq!(
        store.device(device(1)).unwrap().unwrap().removed_at_ms,
        Some(6_000)
    );
}

#[test]
fn removing_an_unknown_device_reports_false_and_touches_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = open(dir.path());
    assert!(!store.remove_device(device(404), 1_000).unwrap());
    assert!(store.list_devices().unwrap().is_empty());
}

#[test]
fn re_registering_a_removed_device_revives_it() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = open(dir.path());
    store.register_device(&new_device(1, 0xAA, 1_000)).unwrap();
    store.remove_device(device(1), 2_000).unwrap();
    assert!(
        store
            .device(device(1))
            .unwrap()
            .unwrap()
            .removed_at_ms
            .is_some()
    );

    // Re-pairing (e.g. the human paired the same physical device again) un-tombstones the row,
    // the same "rejoining revives it" idiom as `upsert_fingerprint`.
    store
        .register_device(&NewDevice {
            key_epoch: 3,
            ..new_device(1, 0xCC, 3_000)
        })
        .unwrap();
    let row = store.device(device(1)).unwrap().unwrap();
    assert!(row.removed_at_ms.is_none());
    assert_eq!(row.static_public, [0xCCu8; 32]);
    assert_eq!(row.key_epoch, 3);
}

#[test]
fn set_device_key_epoch_updates_only_the_epoch() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = open(dir.path());
    store.register_device(&new_device(1, 0xAA, 1_000)).unwrap();

    let existed = store.set_device_key_epoch(device(1), 4).unwrap();
    assert!(existed);
    let row = store.device(device(1)).unwrap().unwrap();
    assert_eq!(row.key_epoch, 4);
    assert_eq!(row.static_public, [0xAAu8; 32], "untouched");

    assert!(!store.set_device_key_epoch(device(404), 1).unwrap());
}
