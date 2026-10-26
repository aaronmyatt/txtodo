//! The removal record (task `peer-prune`): written and read back, and unreadable text is no record.

use txtodo_model::{DeviceId, Ulid};
use txtodo_store::IdentityStore;

use super::{Removal, Why, read, write};
use crate::prune_stale::Stale;

fn store() -> (tempfile::TempDir, IdentityStore) {
    let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
    let store = IdentityStore::open(&dir.path().join("identity.db"))
        .unwrap_or_else(|e| panic!("store: {e}"));
    (dir, store)
}

fn device(n: u128) -> DeviceId {
    DeviceId::new(Ulid::from_u128(n))
}

#[test]
fn every_reason_reads_back_with_who_removed_it() {
    let (_dir, mut store) = store();
    for (n, why) in [
        Why::Manual,
        Why::PrunedQuiet,
        Why::PrunedParked,
        Why::LeftOut,
    ]
    .into_iter()
    .enumerate()
    {
        let (gone, by) = (device(10 + n as u128), device(99));
        write(&mut store, gone, Removal { why, by }).unwrap_or_else(|e| panic!("write: {e}"));
        assert_eq!(read(&store, gone), Some(Removal { why, by }));
    }
}

#[test]
fn a_row_with_no_record_has_none() {
    let (_dir, store) = store();
    assert_eq!(read(&store, device(1)), None);
}

#[test]
fn a_garbled_record_is_no_record() {
    let (_dir, mut store) = store();
    let gone = device(1);
    let key = format!("removed_why:{}", gone.ulid());
    for bad in [
        "",
        "manual",
        "manual:not-a-ulid",
        "mystery:01ARZ3NDEKTSV4RRFFQ69G5FAV",
    ] {
        store
            .meta_set(&key, bad.as_bytes())
            .unwrap_or_else(|e| panic!("meta: {e}"));
        assert_eq!(read(&store, gone), None, "{bad:?}");
    }
}

#[test]
fn a_stale_verdict_maps_to_its_prune_reason() {
    assert_eq!(Why::from(Stale::Quiet), Why::PrunedQuiet);
    assert_eq!(Why::from(Stale::Parked), Why::PrunedParked);
}
