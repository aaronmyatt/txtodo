//! Every `KeyId` variant round-trips through the in-memory store, exhaustively (task notes).

use crate::keystore::{KeyId, KeyStore, Secret};
use crate::keystore_memory::MemoryKeyStore;

fn ids() -> Vec<KeyId> {
    vec![
        KeyId::DeviceSigning,
        KeyId::DeviceStatic,
        KeyId::Group(0),
        KeyId::Group(1),
        KeyId::Group(u32::MAX),
    ]
}

#[test]
fn absent_key_reads_back_none() {
    let store = MemoryKeyStore::new();
    for id in ids() {
        assert_eq!(store.get(id).unwrap(), None);
    }
}

#[test]
fn put_then_get_round_trips_every_variant() {
    let store = MemoryKeyStore::new();
    for (i, id) in ids().into_iter().enumerate() {
        let secret = Secret::new(vec![i as u8; 8]);
        store.put(id, &secret).unwrap();
        let got = store.get(id).unwrap();
        assert_eq!(got, Some(secret));
    }
}

#[test]
fn put_replaces_rather_than_duplicates() {
    let store = MemoryKeyStore::new();
    let id = KeyId::Group(3);
    store.put(id, &Secret::new(vec![1, 1, 1])).unwrap();
    store.put(id, &Secret::new(vec![2, 2, 2])).unwrap();
    assert_eq!(store.get(id).unwrap(), Some(Secret::new(vec![2, 2, 2])));
}

#[test]
fn delete_is_idempotent_and_removes_the_entry() {
    let store = MemoryKeyStore::new();
    let id = KeyId::DeviceSigning;
    store.put(id, &Secret::new(vec![9])).unwrap();
    store.delete(id).unwrap();
    assert_eq!(store.get(id).unwrap(), None);
    // Deleting an absent id is not an error (trait contract).
    store.delete(id).unwrap();
}
