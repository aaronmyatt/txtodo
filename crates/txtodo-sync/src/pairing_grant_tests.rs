//! `PairingGrant`: round-trips through bytes, and `Debug` never prints the group key.

use crate::pairing_grant::PairingGrant;

fn sample() -> PairingGrant {
    PairingGrant {
        group_key: [7u8; 32],
        static_public: [9u8; 32],
        own_device: true,
        epoch: 3,
    }
}

#[test]
fn round_trips_through_bytes() {
    let grant = sample();
    let bytes = grant.to_bytes().unwrap();
    assert_eq!(PairingGrant::from_bytes(&bytes).unwrap(), grant);
    let foreign = PairingGrant {
        own_device: false,
        ..sample()
    };
    let back = PairingGrant::from_bytes(&foreign.to_bytes().unwrap()).unwrap();
    assert!(!back.own_device, "the sender's answer survives the seal");
}

#[test]
fn the_epoch_rides_the_grant() {
    let back = PairingGrant::from_bytes(&sample().to_bytes().unwrap()).unwrap();
    assert_eq!(back.epoch, 3);
}

#[test]
fn a_grant_from_a_build_without_an_epoch_reads_as_epoch_zero() {
    // The old struct, field for field, as postcard encoded it.
    #[derive(serde::Serialize)]
    struct Old {
        group_key: [u8; 32],
        static_public: [u8; 32],
        own_device: bool,
    }
    let old = postcard::to_allocvec(&Old {
        group_key: [7u8; 32],
        static_public: [9u8; 32],
        own_device: true,
    })
    .unwrap();
    let read = PairingGrant::from_bytes(&old).unwrap();
    assert_eq!((read.epoch, read.own_device), (0, true));
    assert_eq!(read.group_key, [7u8; 32]);
    assert!(PairingGrant::from_bytes(&[1, 2, 3]).is_err());
}

#[test]
fn debug_never_prints_the_group_key() {
    let printed = format!("{:?}", sample());
    assert!(printed.contains("<redacted>"));
    assert!(!printed.contains("07070707"), "{printed}");
    // The static public key is not secret, so it is fine for it to appear.
    assert!(printed.contains("0909"), "{printed}");
}
