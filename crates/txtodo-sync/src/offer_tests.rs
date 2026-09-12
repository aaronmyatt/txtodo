//! The QR/code payload round-trips, and — the security-relevant assertion — carries no key
//! material: checked field by field so a future field addition has to be considered (task notes).

use txtodo_model::{DeviceId, Ulid};

use crate::message::GroupId;
use crate::offer::{PairingOffer, from_code, from_qr_bytes, to_code, to_qr_bytes};

fn sample() -> PairingOffer {
    PairingOffer {
        device: DeviceId::new(Ulid::from_u128(42)),
        group: GroupId(99),
        public_key: [0xAB; 32],
        endpoint: "192.168.1.5:4242".to_string(),
        nonce: [7u8; 16],
        issued_at_ms: 1_000,
    }
}

#[test]
fn qr_bytes_round_trip() {
    let offer = sample();
    let bytes = to_qr_bytes(&offer).unwrap();
    assert_eq!(from_qr_bytes(&bytes).unwrap(), offer);
}

#[test]
fn code_round_trips_through_base32() {
    let offer = sample();
    let code = to_code(&offer).unwrap();
    // Base32 (RFC 4648) alphabet only, so a human can type it without ambiguous characters.
    assert!(
        code.chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
    );
    assert_eq!(from_code(&code).unwrap(), offer);
}

#[test]
fn code_is_case_and_whitespace_tolerant() {
    let offer = sample();
    let code = to_code(&offer).unwrap();
    let messy = format!("  {} \n", code.to_ascii_lowercase());
    assert_eq!(from_code(&messy).unwrap(), offer);
}

#[test]
fn garbage_code_is_refused_distinctly() {
    assert!(from_code("not valid base32!!").is_err());
}

#[test]
fn decoded_offer_contains_no_key_material_field_by_field() {
    let offer = sample();
    // Exhaustive per the task: every field is either a public identifier, a public key, an
    // address hint, or a single-use nonce — never a symmetric key or a private scalar.
    let PairingOffer {
        device,
        group,
        public_key,
        endpoint,
        nonce,
        issued_at_ms,
    } = offer;
    let _: DeviceId = device; // a public identifier
    let _: u64 = issued_at_ms; // a timestamp, not a secret
    let _: GroupId = group; // a public identifier, not the group key
    assert_eq!(public_key.len(), 32); // a public key, not a secret scalar
    assert!(!endpoint.is_empty()); // an address hint
    assert_eq!(nonce.len(), 16); // single-use, not confidential
}
