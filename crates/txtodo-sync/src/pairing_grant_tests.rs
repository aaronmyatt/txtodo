//! `PairingGrant`: round-trips through bytes, and `Debug` never prints the group key.

use crate::pairing_grant::PairingGrant;

fn sample() -> PairingGrant {
    PairingGrant {
        group_key: [7u8; 32],
        static_public: [9u8; 32],
    }
}

#[test]
fn round_trips_through_bytes() {
    let grant = sample();
    let bytes = grant.to_bytes().unwrap();
    assert_eq!(PairingGrant::from_bytes(&bytes).unwrap(), grant);
}

#[test]
fn debug_never_prints_the_group_key() {
    let printed = format!("{:?}", sample());
    assert!(printed.contains("<redacted>"));
    assert!(!printed.contains("07070707"), "{printed}");
    // The static public key is not secret, so it is fine for it to appear.
    assert!(printed.contains("0909"), "{printed}");
}
