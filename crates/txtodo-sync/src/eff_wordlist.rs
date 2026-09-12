//! The EFF short wordlist (dice-generated passphrases, list 1), vendored verbatim as one word per
//! line in publication order. Ref: <https://www.eff.org/dice> ·
//! <https://www.eff.org/files/2016/09/08/eff_short_wordlist_1.txt>.
//!
//! A SAS is only as trustworthy as the list it is drawn from: if the file silently changed, every
//! SAS this build produces would silently change with it, and the two humans comparing words would
//! never notice they were comparing against different lists. `wordlist_tests.rs` pins the shape
//! (exactly 1296 entries, no duplicates) and this file's exact bytes (a SHA-256).

use std::sync::OnceLock;

const RAW: &str = include_str!("wordlists/eff_short_wordlist_1.txt");

/// Entries in the list: EFF's short list is `6^4`, sized for four six-sided dice per word.
pub const WORDLIST_LEN: usize = 1296;

/// SHA-256 of the vendored file (`wordlists/eff_short_wordlist_1.txt`), lowercase hex. Asserted by
/// a test so an edit to the file — accidental or not — is caught rather than silently changing
/// every SAS this build produces.
pub const WORDLIST_SHA256: &str =
    "36ecca49e4fa20ca84b176c32f2e9c82f98f446585190e75f9879a95c08247bf";

fn words() -> &'static Vec<&'static str> {
    static WORDS: OnceLock<Vec<&'static str>> = OnceLock::new();
    WORDS.get_or_init(|| RAW.lines().filter(|l| !l.is_empty()).collect())
}

/// The full list, in file order. `index` in [`crate::sas::sas_words`] is a position into this.
pub fn wordlist() -> &'static [&'static str] {
    let w = words();
    debug_assert_eq!(w.len(), WORDLIST_LEN);
    w
}
