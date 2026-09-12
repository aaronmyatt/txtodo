//! Signature tests: round trip, one flipped byte, whole-batch refusal, and the redaction rules.
//! Keys here are fixtures — the keystore task owns the real ones.

use std::collections::BTreeMap;

use txtodo_model::{DeviceId, FilePath, Hlc, Op, OpId, OpKind, Principal, TaskId, Ulid};

use crate::crypto_error::CryptoError;
use crate::sign::{DevicePublicKey, DeviceSigningKey, Signature, sign, verify, verify_batch};

fn dev(n: u128) -> DeviceId {
    DeviceId::new(Ulid::from_u128(n))
}

fn op(device: DeviceId, line: &str) -> Op {
    // The id does not enter the signature test meaning; a per-device value keeps rows distinct.
    let n = device.ulid().to_u128();
    Op {
        id: OpId::new(Ulid::from_u128(0x0100 + n)),
        hlc: Hlc {
            wall_ms: 1_700_000_000_000,
            counter: 0,
            device,
        },
        principal: Principal::User { device },
        file: FilePath::new("todo.txt").unwrap(),
        kind: OpKind::Insert {
            task: TaskId::new(Ulid::from_u128(0x0200)),
            after: None,
            line: line.to_owned(),
        },
    }
}

fn key(seed: u8) -> DeviceSigningKey {
    DeviceSigningKey::from_bytes([seed; 32])
}

fn keys(entries: &[(DeviceId, DeviceSigningKey)]) -> BTreeMap<DeviceId, DevicePublicKey> {
    entries.iter().map(|(d, k)| (*d, k.public_key())).collect()
}

#[test]
fn sign_and_verify_round_trip_including_the_public_key_bytes() {
    let device = dev(1);
    let k = key(7);
    let public = k.public_key();
    assert_eq!(
        DevicePublicKey::from_bytes(public.to_bytes()).unwrap(),
        public
    );
    let signed = sign(&op(device, "buy milk"), &k).unwrap();
    assert!(verify(&op(device, "buy milk"), &signed, &public).is_ok());
}

#[test]
fn flipping_one_byte_of_a_signed_op_fails_verification() {
    let device = dev(1);
    let k = key(7);
    let signed = sign(&op(device, "buy milk"), &k).unwrap();
    // One byte of the line text differs; signing_bytes covers it, so the tag cannot match.
    let tampered = op(device, "buy silk");
    assert_eq!(
        verify(&tampered, &signed, &k.public_key()),
        Err(CryptoError::SignatureInvalid { device })
    );
}

#[test]
fn tampering_the_signature_itself_fails_verification() {
    let device = dev(1);
    let k = key(7);
    let mut bytes = sign(&op(device, "buy milk"), &k).unwrap().to_bytes();
    bytes[0] ^= 0x01;
    let tampered = Signature::from_bytes(bytes);
    assert_eq!(
        verify(&op(device, "buy milk"), &tampered, &k.public_key()),
        Err(CryptoError::SignatureInvalid { device })
    );
}

#[test]
fn a_signature_from_another_device_key_does_not_verify() {
    let device = dev(1);
    let signed = sign(&op(device, "buy milk"), &key(7)).unwrap();
    assert_eq!(
        verify(&op(device, "buy milk"), &signed, &key(9).public_key()),
        Err(CryptoError::SignatureInvalid { device })
    );
}

#[test]
fn verify_batch_rejects_the_whole_batch_on_one_bad_signature() {
    let a = dev(1);
    let b = dev(2);
    let ka = key(7);
    let kb = key(8);
    let ops = vec![op(a, "one"), op(b, "two"), op(a, "three")];
    let mut sigs: Vec<Signature> = ops
        .iter()
        .map(|o| {
            let k = if o.hlc.device == a { &ka } else { &kb };
            sign(o, k).unwrap()
        })
        .collect();
    let known = keys(&[(a, key(7)), (b, key(8))]);
    assert!(
        verify_batch(&ops, &sigs, &known).is_ok(),
        "control batch passes"
    );
    // The middle op's line is altered after signing: that one signature no longer matches.
    let mut corrupted = ops.clone();
    corrupted[1] = op(b, "TWO");
    assert_eq!(
        verify_batch(&corrupted, &sigs, &known),
        Err(CryptoError::SignatureInvalid { device: b })
    );
    sigs[1] = Signature::from_bytes([0u8; 64]);
    assert!(verify_batch(&ops, &sigs, &known).is_err(), "still refused");
}

#[test]
fn verify_batch_refuses_a_length_mismatch_before_verifying_anything() {
    let a = dev(1);
    let op = op(a, "one");
    let sig = sign(&op, &key(7)).unwrap();
    let known = keys(&[(a, key(7))]);
    assert_eq!(
        verify_batch(&[op], &[sig, sig], &known),
        Err(CryptoError::BatchLength {
            ops: 1,
            signatures: 2
        })
    );
}

#[test]
fn verify_batch_names_an_unknown_device() {
    let a = dev(1);
    let op = op(a, "one");
    let sig = sign(&op, &key(7)).unwrap();
    // No entry for `a`: a missing key is refused, never skipped.
    assert_eq!(
        verify_batch(&[op], &[sig], &BTreeMap::new()),
        Err(CryptoError::UnknownDevice { device: a })
    );
}

#[test]
fn a_signing_key_debug_never_prints_the_seed() {
    let k = key(0xAB);
    assert_eq!(format!("{k:?}"), "DeviceSigningKey(<redacted>)");
    assert!(!format!("{k:?}").contains("ab"), "no key bytes in the line");
}

#[test]
fn a_signature_debug_shows_a_prefix_not_all_64_bytes() {
    let sig = Signature::from_bytes([0xCD; 64]);
    assert_eq!(format!("{sig:?}"), "Signature(cdcd..)");
}

#[test]
fn a_signature_round_trips_through_postcard() {
    let sig = Signature::from_bytes([0x11; 64]);
    let bytes = postcard::to_allocvec(&sig).unwrap();
    let back: Signature = postcard::from_bytes(&bytes).unwrap();
    assert_eq!(back, sig);
}

#[test]
fn a_public_key_that_is_not_a_point_is_a_typed_error() {
    // Not every 32-byte string is a compressed Edwards point; 0x02 repeated is not.
    assert_eq!(
        DevicePublicKey::from_bytes([0x02; 32]),
        Err(CryptoError::BadPublicKey { device: None })
    );
}
