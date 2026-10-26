//! `group_epoch.rs` against an in-memory keystore.

use super::*;
use txtodo_sync::{MemoryKeyStore, Secret};

fn put(store: &MemoryKeyStore, epoch: u32, byte: u8) {
    store
        .put(KeyId::Group(epoch), &Secret::new(vec![byte; 32]))
        .unwrap();
}

#[test]
fn the_current_key_and_every_older_one_held_are_retained() {
    let store = MemoryKeyStore::new();
    put(&store, 0, 1);
    put(&store, 2, 3); // epoch 1 was never held here
    let keys = retained(&store, 2).unwrap();
    assert_eq!(keys.len(), 2);
    assert!(keys.get(0).is_some() && keys.get(1).is_none() && keys.get(2).is_some());
}

#[test]
fn without_the_current_epochs_key_there_is_nothing_to_seal_under() {
    let store = MemoryKeyStore::new();
    put(&store, 0, 1);
    assert!(retained(&store, 1).is_none());
}

#[test]
fn only_the_newest_sixteen_epochs_are_kept() {
    let store = MemoryKeyStore::new();
    for epoch in 0..=20 {
        put(&store, epoch, 7);
    }
    let keys = retained(&store, 20).unwrap();
    assert_eq!(keys.len(), MAX_RETAINED_KEY_EPOCHS);
    assert!(keys.get(4).is_none() && keys.get(5).is_some());
}

#[test]
fn a_stored_key_of_the_wrong_length_is_an_error_a_missing_one_is_not() {
    let store = MemoryKeyStore::new();
    store
        .put(KeyId::Group(0), &Secret::new(vec![1; 5]))
        .unwrap();
    assert!(fetch(&store, 0).unwrap_err().contains("5 bytes"));
    assert!(fetch(&store, 1).unwrap().is_none());
}

#[test]
fn a_batch_is_verified_with_the_key_of_its_own_epoch() {
    let store = MemoryKeyStore::new();
    put(&store, 0, 1);
    put(&store, 1, 2);
    let keys = retained(&store, 1).unwrap();
    let (old, new) = (verify_key(&keys, 0).unwrap(), verify_key(&keys, 1).unwrap());
    assert_ne!(
        old.to_bytes(),
        new.to_bytes(),
        "each epoch signs with its own key"
    );
    assert!(verify_key(&keys, 5).is_none());
}

#[test]
fn a_new_group_epoch_ends_every_live_session_and_the_same_one_does_not() {
    let dir = tempfile::tempdir().unwrap();
    let identity = crate::device_identity::DeviceIdentity::open_in_memory(
        dir.path(),
        &crate::clock::FakeClock::new(1_000),
    )
    .unwrap();
    let before = identity.live_peers().own_generation();
    identity.set_group_epoch(0);
    assert_eq!(identity.live_peers().own_generation(), before, "unchanged");
    identity.set_group_epoch(1);
    assert_ne!(
        identity.live_peers().own_generation(),
        before,
        "sessions greet again, sealing under epoch 1"
    );
    assert_eq!(identity.group_epoch(), 1);
}
