//! Wordlist invariants: count, duplicates, file hash (task notes).

use sha2::{Digest, Sha256};

use crate::eff_wordlist::{WORDLIST_LEN, WORDLIST_SHA256, wordlist};

#[test]
fn has_exactly_1296_entries() {
    assert_eq!(wordlist().len(), WORDLIST_LEN);
    assert_eq!(WORDLIST_LEN, 1296);
}

#[test]
fn has_no_duplicates() {
    let words = wordlist();
    let unique: std::collections::BTreeSet<&str> = words.iter().copied().collect();
    assert_eq!(unique.len(), words.len());
}

#[test]
fn every_entry_is_nonempty_lowercase_ascii() {
    for w in wordlist() {
        assert!(!w.is_empty());
        assert!(w.chars().all(|c| c.is_ascii_lowercase() || c == '-'));
    }
}

#[test]
fn vendored_file_matches_the_pinned_hash() {
    let raw = include_str!("wordlists/eff_short_wordlist_1.txt");
    let digest = Sha256::digest(raw.as_bytes());
    let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    assert_eq!(hex, WORDLIST_SHA256);
}
