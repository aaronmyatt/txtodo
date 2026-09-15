//! AEAD tests: round trip, fresh nonces, tamper detection, and the three context failures
//! (wrong key, wrong group, wrong epoch) each giving their own typed error.

use crate::aead::{GroupKey, GroupKeys, MAX_RETAINED_KEY_EPOCHS, SEALED_HEADER_BYTES, open, seal};
use crate::crypto_error::CryptoError;
use crate::frame::PROTOCOL_VERSION;
use crate::message::GroupId;

fn key(byte: u8) -> GroupKey {
    GroupKey::from_bytes([byte; 32])
}

fn one_epoch(epoch: u32, key_byte: u8) -> GroupKeys {
    let mut keys = GroupKeys::new();
    keys.insert(epoch, key(key_byte)).unwrap();
    keys
}

#[test]
fn seal_and_open_round_trip_with_the_header_in_the_clear() {
    let group = GroupId(0xABCD);
    let keys = one_epoch(3, 7);
    let sealed = seal(
        PROTOCOL_VERSION,
        group,
        3,
        keys.get(3).unwrap(),
        b"ops batch",
    )
    .unwrap();
    assert_eq!(u16::from_le_bytes([sealed[0], sealed[1]]), PROTOCOL_VERSION);
    assert_eq!(sealed.len(), SEALED_HEADER_BYTES + b"ops batch".len() + 16);
    assert_eq!(
        open(PROTOCOL_VERSION, group, &keys, &sealed).unwrap(),
        b"ops batch"
    );
}

#[test]
fn nonces_are_fresh_from_the_os_and_not_a_seeded_prng() {
    // `seal` takes no RNG argument, so the deterministic simulator cannot inject one; the observable
    // proof is that 100 seals of identical bytes never repeat a header.
    let group = GroupId(1);
    let keys = one_epoch(0, 7);
    let mut headers = std::collections::BTreeSet::new();
    for _ in 0..100 {
        let sealed = seal(PROTOCOL_VERSION, group, 0, keys.get(0).unwrap(), b"same").unwrap();
        assert!(headers.insert(sealed[..SEALED_HEADER_BYTES].to_vec()));
    }
    assert_eq!(headers.len(), 100);
}

#[test]
fn flipping_one_ciphertext_byte_fails_the_tag() {
    let group = GroupId(1);
    let keys = one_epoch(0, 7);
    let mut sealed = seal(PROTOCOL_VERSION, group, 0, keys.get(0).unwrap(), b"ops").unwrap();
    let last = sealed.len() - 1;
    sealed[last] ^= 0x01;
    assert_eq!(
        open(PROTOCOL_VERSION, group, &keys, &sealed),
        Err(CryptoError::Decrypt { epoch: 0 })
    );
}

#[test]
fn a_wrong_group_key_fails_the_tag_but_an_unknown_epoch_is_a_distinct_error() {
    let group = GroupId(1);
    // Same epoch number, different key bytes: the header parses, the tag fails.
    assert_eq!(
        open(
            PROTOCOL_VERSION,
            group,
            &one_epoch(0, 8),
            &seal(PROTOCOL_VERSION, group, 0, &key(7), b"ops").unwrap()
        ),
        Err(CryptoError::Decrypt { epoch: 0 })
    );
    // No key for epoch 5 at all: named, not guessed at.
    assert_eq!(
        open(
            PROTOCOL_VERSION,
            group,
            &one_epoch(0, 7),
            &seal(PROTOCOL_VERSION, group, 5, &key(7), b"ops").unwrap()
        ),
        Err(CryptoError::UnknownEpoch { epoch: 5, held: 1 })
    );
}

#[test]
fn a_foreign_group_fails_the_aad_before_the_ciphertext_is_touched() {
    let sealed = seal(PROTOCOL_VERSION, GroupId(1), 0, &key(7), b"ops").unwrap();
    assert_eq!(
        open(PROTOCOL_VERSION, GroupId(2), &one_epoch(0, 7), &sealed),
        Err(CryptoError::WrongGroup {
            got: GroupId(1),
            expected: GroupId(2)
        })
    );
}

#[test]
fn a_downgraded_version_fails_the_aad_distinctly() {
    let sealed = seal(PROTOCOL_VERSION, GroupId(1), 0, &key(7), b"ops").unwrap();
    assert_eq!(
        open(PROTOCOL_VERSION + 1, GroupId(1), &one_epoch(0, 7), &sealed),
        Err(CryptoError::WrongVersion {
            got: PROTOCOL_VERSION,
            expected: PROTOCOL_VERSION + 1
        })
    );
}

#[test]
fn a_short_blob_is_refused_before_any_indexing() {
    let keys = one_epoch(0, 7);
    for len in 0..(SEALED_HEADER_BYTES + 16) {
        assert_eq!(
            open(PROTOCOL_VERSION, GroupId(1), &keys, &vec![0u8; len]),
            Err(CryptoError::Truncated {
                needed: SEALED_HEADER_BYTES + 16,
                got: len
            }),
            "len {len}"
        );
    }
}

#[test]
fn retaining_more_than_the_cap_is_refused_and_replacing_is_allowed() {
    let mut keys = GroupKeys::new();
    for epoch in 0..MAX_RETAINED_KEY_EPOCHS as u32 {
        keys.insert(epoch, key(epoch as u8)).unwrap();
    }
    assert_eq!(keys.len(), MAX_RETAINED_KEY_EPOCHS);
    assert_eq!(
        keys.insert(999, key(1)),
        Err(CryptoError::TooManyEpochs {
            len: MAX_RETAINED_KEY_EPOCHS + 1,
            max: MAX_RETAINED_KEY_EPOCHS
        })
    );
    // Overwriting an epoch already held never grows the set.
    assert!(keys.insert(0, key(0xFF)).is_ok());
    assert_eq!(keys.len(), MAX_RETAINED_KEY_EPOCHS);
    assert!(keys.get(0).is_some());
    assert!(keys.get(999).is_none());
    assert!(!keys.is_empty());
}

#[test]
fn a_group_key_debug_never_prints_key_material() {
    let k = key(0xAB);
    assert_eq!(format!("{k:?}"), "GroupKey(<redacted>)");
    assert!(!format!("{k:?}").contains("ab"));
}
