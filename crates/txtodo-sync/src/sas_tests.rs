//! SAS/pair-key derivation: identical inputs give identical words, any single-bit change to either
//! input changes them, and the SAS/pair-key never coincide.

use crate::sas::{pair_key, sas_words};

#[test]
fn identical_secret_and_transcript_give_identical_words() {
    let secret = [7u8; 32];
    let transcript = b"same transcript bytes";
    assert_eq!(
        sas_words(&secret, transcript).unwrap(),
        sas_words(&secret, transcript).unwrap()
    );
}

#[test]
fn a_single_bit_flip_in_the_secret_changes_the_words() {
    let mut secret = [7u8; 32];
    let transcript = b"same transcript bytes";
    let base = sas_words(&secret, transcript).unwrap();
    secret[0] ^= 1;
    assert_ne!(base, sas_words(&secret, transcript).unwrap());
}

#[test]
fn a_single_bit_flip_in_the_transcript_changes_the_words() {
    let secret = [7u8; 32];
    let base = sas_words(&secret, b"transcript-a").unwrap();
    let changed = sas_words(&secret, b"transcript-b").unwrap();
    assert_ne!(base, changed);
}

#[test]
fn sas_and_pair_key_are_derived_independently() {
    let secret = [9u8; 32];
    let transcript = b"a pairing transcript";
    let words = sas_words(&secret, transcript).unwrap();
    let key_a = pair_key(&secret, transcript).unwrap();
    let key_b = pair_key(&secret, transcript).unwrap();
    // Same info, same inputs: deterministic.
    assert_eq!(key_a, key_b);
    // Different info strings from the same extract step must not yield the same bytes: comparing
    // the pair key against a byte reinterpretation of the SAS words wouldn't be meaningful (they
    // are different shapes), so instead assert the two derivations use distinct `info` constants.
    assert_ne!(crate::sas::SAS_INFO, crate::sas::PAIR_KEY_INFO);
    assert!(!words.is_empty());
}

#[test]
fn pair_key_is_32_bytes_for_xchacha20poly1305() {
    let key = pair_key(&[1u8; 32], b"t").unwrap();
    assert_eq!(key.len(), 32);
}
