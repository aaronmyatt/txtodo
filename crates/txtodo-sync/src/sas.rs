//! Deriving the short authentication string and the key-wrap key from one X25519 shared secret and
//! the pairing [`crate::transcript`]. One HKDF-SHA256 extract, two expands under different `info`
//! strings — so the SAS a human reads and the key that later wraps the group key are provably
//! independent, never the same derived bytes reused for two purposes.
//! Ref: <https://datatracker.ietf.org/doc/html/rfc5869>.

use std::fmt;

use hkdf::Hkdf;
use sha2::Sha256;

use crate::eff_wordlist::wordlist;

/// Words shown for human comparison: `6 * log2(1296) ≈ 62` bits, per the task.
pub const SAS_WORD_COUNT: usize = 6;
/// `HKDF-Expand` info for the SAS. Never reused for anything else derived from the same secret.
pub const SAS_INFO: &[u8] = b"txtodo-sas-v1";
/// `HKDF-Expand` info for the key that wraps the group key. Distinct from [`SAS_INFO`] so the two
/// outputs cannot be confused, swapped, or used to recover one another.
pub const PAIR_KEY_INFO: &[u8] = b"txtodo-pair-v1";
/// Bytes in the derived key-wrap key (XChaCha20-Poly1305, matching the rest of the wire).
pub const PAIR_KEY_BYTES: usize = 32;

/// HKDF refused to produce the requested output length. Unreachable in practice at these output
/// sizes (RFC 5869 allows up to 255 hash-lengths), but modelled as a typed error rather than a
/// panic per CLAUDE.md §3: nothing here assumes an infallible library call.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct SasError;

impl fmt::Display for SasError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "HKDF-SHA256 refused to expand to the requested length")
    }
}

impl std::error::Error for SasError {}

/// One extract-then-expand call, so both derived values below share the same extract step and
/// differ only in `info`.
fn expand(
    shared_secret: &[u8; 32],
    transcript: &[u8],
    info: &[u8],
    out: &mut [u8],
) -> Result<(), SasError> {
    let hk = Hkdf::<Sha256>::new(Some(transcript), shared_secret);
    hk.expand(info, out).map_err(|_| SasError)
}

/// Six words from the EFF short list. Each word's candidate is a little-endian `u32` read from its
/// own 4-byte chunk of the `SAS_WORD_COUNT * 4`-byte `HKDF-Expand(info = "txtodo-sas-v1")` output,
/// taken modulo the wordlist length (1296). 1296 does not divide `2^32`, so this slightly favours
/// the lowest `2^32 mod 1296` indices; the bias is at most `1296 / 2^32 ≈ 3 × 10⁻⁷` per word, far
/// below what six words' ~62 bits need to resist guessing inside one pairing window.
pub fn sas_words(
    shared_secret: &[u8; 32],
    transcript: &[u8],
) -> Result<[&'static str; SAS_WORD_COUNT], SasError> {
    let mut okm = [0u8; SAS_WORD_COUNT * 4];
    expand(shared_secret, transcript, SAS_INFO, &mut okm)?;
    let list = wordlist();
    let mut words = [""; SAS_WORD_COUNT];
    for (i, word) in words.iter_mut().enumerate() {
        let chunk = [okm[i * 4], okm[i * 4 + 1], okm[i * 4 + 2], okm[i * 4 + 3]];
        let idx = (u32::from_le_bytes(chunk) as usize) % list.len();
        *word = list[idx];
    }
    debug_assert!(words.iter().all(|w| !w.is_empty()));
    Ok(words)
}

/// The key that wraps the group key once both sides confirm the SAS.
pub fn pair_key(
    shared_secret: &[u8; 32],
    transcript: &[u8],
) -> Result<[u8; PAIR_KEY_BYTES], SasError> {
    let mut out = [0u8; PAIR_KEY_BYTES];
    expand(shared_secret, transcript, PAIR_KEY_INFO, &mut out)?;
    debug_assert_ne!(out, [0u8; PAIR_KEY_BYTES]);
    Ok(out)
}
