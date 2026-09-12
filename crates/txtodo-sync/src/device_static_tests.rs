//! `DeviceStaticSecret`/`DeviceStaticPublic`: round-trip, redacted `Debug`, and the shared
//! ECDH secret two independently-generated keys agree on.

use crate::device_static::{DeviceStaticPublic, DeviceStaticSecret};

#[test]
fn to_bytes_and_from_bytes_round_trip() {
    let secret = DeviceStaticSecret::generate();
    let bytes = secret.to_bytes();
    let reloaded = DeviceStaticSecret::from_bytes(bytes);
    assert_eq!(reloaded.to_bytes(), bytes);
}

#[test]
fn public_key_round_trips_through_bytes() {
    let secret = DeviceStaticSecret::generate();
    let public = secret.public_key();
    assert_eq!(DeviceStaticPublic::from_bytes(public.to_bytes()), public);
}

#[test]
fn debug_never_prints_key_bytes() {
    let secret = DeviceStaticSecret::generate();
    let printed = format!("{secret:?}");
    assert_eq!(printed, "DeviceStaticSecret(<redacted>)");
    let hex: String = secret
        .to_bytes()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    assert!(!printed.contains(&hex));
}

#[test]
fn two_devices_agree_on_the_same_ecdh_secret() {
    let a = DeviceStaticSecret::generate();
    let b = DeviceStaticSecret::generate();
    let from_a = a.diffie_hellman_with(&b.public_key().to_bytes());
    let from_b = b.diffie_hellman_with(&a.public_key().to_bytes());
    assert_eq!(from_a, from_b);
}
