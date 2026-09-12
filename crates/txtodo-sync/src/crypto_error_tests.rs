//! The error type is the contract with logs: every variant must say what failed and against which
//! epoch or device. These tests pin the wording that operators grep for.

use txtodo_model::{DeviceId, Ulid};

use crate::crypto_error::CryptoError;
use crate::message::GroupId;

fn dev(n: u128) -> DeviceId {
    DeviceId::new(Ulid::from_u128(n))
}

#[test]
fn errors_name_the_device_epoch_or_group_they_failed_against() {
    let device = dev(7);
    let cases = [
        (
            CryptoError::UnknownDevice { device },
            format!("no verifying key held for device {device}"),
        ),
        (
            CryptoError::SignatureInvalid { device },
            format!("signature from device {device} did not verify"),
        ),
        (
            CryptoError::UnknownEpoch { epoch: 5, held: 2 },
            "no group key retained for epoch 5 (2 held)".to_owned(),
        ),
        (
            CryptoError::WrongGroup {
                got: GroupId(1),
                expected: GroupId(2),
            },
            "sealed batch is for group 1, expected 2".to_owned(),
        ),
        (
            CryptoError::BatchLength {
                ops: 3,
                signatures: 2,
            },
            "batch has 3 ops but 2 signatures; refusing to verify partially".to_owned(),
        ),
        (
            CryptoError::TooManyEpochs { len: 17, max: 16 },
            "retaining 17 group-key epochs exceeds the cap of 16".to_owned(),
        ),
        (
            CryptoError::Decrypt { epoch: 4 },
            "AEAD tag failed for epoch 4: wrong key or tampered".to_owned(),
        ),
        (
            CryptoError::Entropy,
            "the OS CSPRNG failed; no nonce was produced".to_owned(),
        ),
    ];
    for (error, want) in cases {
        assert_eq!(error.to_string(), want, "{error:?}");
    }
}

#[test]
fn a_crypto_error_is_a_std_error_so_a_caller_can_box_it() {
    let boxed: Box<dyn std::error::Error> = Box::new(CryptoError::BadPublicKey { device: None });
    assert!(boxed.to_string().contains("Ed25519 point"));
    assert!(boxed.source().is_none(), "these variants carry no source");
}
