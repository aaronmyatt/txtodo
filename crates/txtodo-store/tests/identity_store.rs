//! Device-set identity's raw store (ADR 0021, task `daemon-device-set-identity`): meta
//! round-trips and the same register/list/remove-tombstones-not-deletes discipline `tests/
//! devices.rs` covers for the per-workspace table — this is its device-global twin, same schema.
// Integration tests are tests: clippy.toml allows unwrap/expect in #[test] fns but not in their helpers.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::path::Path;
use txtodo_model::{DeviceId, Ulid};
use txtodo_store::{IdentityStore, NewDevice};

fn open(dir: &Path) -> IdentityStore {
    IdentityStore::open(&dir.join("identity.db")).unwrap_or_else(|e| panic!("open: {e}"))
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
fn meta_set_and_get_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let mut identity = open(dir.path());
    assert!(identity.meta_get("device_id").unwrap().is_none());
    identity.meta_set("device_id", b"some-bytes").unwrap();
    assert_eq!(
        identity.meta_get("device_id").unwrap(),
        Some(b"some-bytes".to_vec())
    );
}

#[test]
fn meta_persists_across_a_fresh_open_restart_durability() {
    let dir = tempfile::tempdir().unwrap();
    {
        let mut identity = open(dir.path());
        identity.meta_set("group_id", b"a-group").unwrap();
    }
    let reopened = open(dir.path());
    assert_eq!(
        reopened.meta_get("group_id").unwrap(),
        Some(b"a-group".to_vec())
    );
}

#[test]
fn register_list_and_get_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let mut identity = open(dir.path());
    identity
        .register_device(&new_device(1, 0xAA, 1_000))
        .unwrap();
    identity
        .register_device(&new_device(2, 0xBB, 2_000))
        .unwrap();

    let listed = identity.list_devices().unwrap();
    assert_eq!(listed.len(), 2);
    assert_eq!(listed[0].device, device(1), "oldest paired_at first");
    assert_eq!(listed[0].static_public, [0xAAu8; 32]);
    assert!(listed[0].removed_at_ms.is_none());

    let by_id = identity.device(device(2)).unwrap().expect("found by id");
    assert_eq!(by_id.name, "device-2");
    assert!(identity.device(device(404)).unwrap().is_none());
}

#[test]
fn removing_a_device_tombstones_it_rather_than_deleting_it() {
    let dir = tempfile::tempdir().unwrap();
    let mut identity = open(dir.path());
    identity
        .register_device(&new_device(1, 0xAA, 1_000))
        .unwrap();

    assert!(identity.remove_device(device(1), 5_000).unwrap());
    let row = identity.device(device(1)).unwrap().unwrap();
    assert_eq!(row.removed_at_ms, Some(5_000));
    assert_eq!(row.name, "device-1", "row kept, not deleted");

    // Idempotent: removing an already-removed device still reports true.
    assert!(identity.remove_device(device(1), 6_000).unwrap());
    // An id this store never heard of reports false.
    assert!(!identity.remove_device(device(404), 1_000).unwrap());
}

#[test]
fn set_device_key_epoch_updates_only_the_epoch_and_reports_false_for_an_unknown_device() {
    let dir = tempfile::tempdir().unwrap();
    let mut identity = open(dir.path());
    identity
        .register_device(&new_device(1, 0xAA, 1_000))
        .unwrap();

    assert!(identity.set_device_key_epoch(device(1), 3).unwrap());
    let row = identity.device(device(1)).unwrap().unwrap();
    assert_eq!(row.key_epoch, 3);
    assert_eq!(row.name, "device-1", "only the epoch changed");

    assert!(!identity.set_device_key_epoch(device(404), 1).unwrap());
}

#[test]
fn set_relay_reachability_round_trips_and_reports_false_for_an_unknown_device() {
    let dir = tempfile::tempdir().unwrap();
    let mut identity = open(dir.path());
    identity
        .register_device(&new_device(1, 0xAA, 1_000))
        .unwrap();

    let node_id = [7u8; 32];
    assert!(
        identity
            .set_relay_reachability(device(1), node_id, "https://relay.example.org")
            .unwrap()
    );
    let row = identity.device(device(1)).unwrap().unwrap();
    assert_eq!(row.relay_node_id, Some(node_id));
    assert_eq!(row.relay_url.as_deref(), Some("https://relay.example.org"));

    assert!(
        !identity
            .set_relay_reachability(device(404), node_id, "https://relay.example.org")
            .unwrap()
    );
}
