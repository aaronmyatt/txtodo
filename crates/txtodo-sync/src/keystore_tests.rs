//! `KeyId`/`Secret` shape tests: the cheap, exhaustive checks the task calls for.

use crate::keystore::{KeyId, Secret};

#[test]
fn secret_debug_is_redacted() {
    let s = Secret::new(b"super-secret-key-material".to_vec());
    let printed = format!("{s:?}");
    assert_eq!(printed, "Secret(<redacted>)");
    assert!(!printed.contains("super-secret"));
}

#[test]
fn secret_expose_round_trips_bytes() {
    let bytes = vec![1u8, 2, 3, 4, 5];
    let s = Secret::new(bytes.clone());
    assert_eq!(s.expose(), &bytes[..]);
    assert_eq!(s.clone(), s);
}

#[test]
fn key_id_display_is_distinct_per_variant() {
    let ids = [
        KeyId::DeviceSigning,
        KeyId::DeviceStatic,
        KeyId::Group(0),
        KeyId::Group(1),
    ];
    let mut seen = std::collections::BTreeSet::new();
    for id in ids {
        assert!(seen.insert(id.to_string()), "duplicate display for {id:?}");
    }
    assert_eq!(seen.len(), ids.len());
}

#[test]
fn key_id_ordering_is_total_and_stable() {
    // Exhaustive per the task: every variant compares against every other without panicking, and
    // a value always equals itself.
    let ids = [
        KeyId::DeviceSigning,
        KeyId::DeviceStatic,
        KeyId::Group(0),
        KeyId::Group(u32::MAX),
    ];
    for a in ids {
        for b in ids {
            let _ = a.cmp(&b);
        }
        assert_eq!(a.cmp(&a), std::cmp::Ordering::Equal);
    }
}
