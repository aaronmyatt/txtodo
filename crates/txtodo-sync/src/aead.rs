//! Batch confidentiality: XChaCha20-Poly1305 with the group key. The signature says *who* wrote an
//! op; this says *only the group may read it in flight*. The seal is stripped at import and the log
//! on disk is plaintext (the SQLite file is already as secret as the todo.txt beside it).
//!
//! XChaCha's 24-byte nonce is the whole point: random nonces are safe at our volumes, so there is no
//! counter to persist and no way to reuse one after a restore-from-backup. The nonce is drawn fresh
//! per batch from the OS CSPRNG, **never** from an injectable/seeded PRNG, so the deterministic
//! simulator (`tests/sim.rs`) cannot reach this path.
//!
//! The clear header binds the context: `version || group || epoch || nonce`. The same
//! `version || group || epoch` bytes are the AEAD associated data, so a foreign group or a downgrade
//! fails the tag check rather than decrypting into something plausible. `epoch` names the group-key
//! generation (`txtodo device remove` rotates it); an unknown epoch is a typed error naming it,
//! never a try-every-key loop.
//!
//! Refs: <https://docs.rs/chacha20poly1305> · XChaCha draft
//! <https://datatracker.ietf.org/doc/html/draft-irtf-cfrg-xchacha-03> · AEAD concept
//! <https://datatracker.ietf.org/doc/html/rfc5116>.

use std::collections::BTreeMap;
use std::fmt;

use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};

use crate::crypto_error::CryptoError;
use crate::message::GroupId;

/// Bytes in a group key (XChaCha20-Poly1305).
pub const KEY_BYTES: usize = 32;
/// Bytes in an XChaCha nonce; large enough that random nonces never collide in practice.
pub const NONCE_BYTES: usize = 24;
/// Bytes Poly1305 appends to the ciphertext.
pub const TAG_BYTES: usize = 16;
/// Bytes of `version || group || epoch` bound as associated data.
pub const AAD_BYTES: usize = 2 + 16 + 4;
/// Clear sealed-batch header: `version (u16) || group (u128) || epoch (u32) || nonce (24)`.
pub const SEALED_HEADER_BYTES: usize = AAD_BYTES + NONCE_BYTES;
/// Group-key generations kept for reading history after a rotation. Old ops stay under their old key.
pub const MAX_RETAINED_KEY_EPOCHS: usize = 16;

/// One group key for one epoch. Opaque and redacted: never let a `Debug` print key material.
#[derive(Clone, PartialEq, Eq)]
pub struct GroupKey([u8; KEY_BYTES]);

impl GroupKey {
    /// Wraps raw key bytes from the keystore.
    pub const fn from_bytes(bytes: [u8; KEY_BYTES]) -> GroupKey {
        GroupKey(bytes)
    }

    fn cipher(&self) -> XChaCha20Poly1305 {
        XChaCha20Poly1305::new((&self.0).into())
    }
}

impl fmt::Debug for GroupKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("GroupKey(<redacted>)")
    }
}

/// The group keys this device retains, one per epoch. A `BTreeMap` so iteration order is stable and
/// nothing hash-ordered ever touches key material.
#[derive(Clone, Debug, Default)]
pub struct GroupKeys {
    keys: BTreeMap<u32, GroupKey>,
}

impl GroupKeys {
    /// An empty set; the caller inserts the current epoch after pairing.
    pub fn new() -> GroupKeys {
        GroupKeys {
            keys: BTreeMap::new(),
        }
    }

    /// Adds a key for `epoch`, refusing to retain more than `MAX_RETAINED_KEY_EPOCHS`. Replacing an
    /// existing epoch never grows the set, so a rotation may overwrite.
    pub fn insert(&mut self, epoch: u32, key: GroupKey) -> Result<(), CryptoError> {
        if !self.keys.contains_key(&epoch) && self.keys.len() >= MAX_RETAINED_KEY_EPOCHS {
            return Err(CryptoError::TooManyEpochs {
                len: self.keys.len() + 1,
                max: MAX_RETAINED_KEY_EPOCHS,
            });
        }
        self.keys.insert(epoch, key);
        debug_assert!(self.keys.len() <= MAX_RETAINED_KEY_EPOCHS);
        Ok(())
    }

    /// The key for `epoch`, if retained.
    pub fn get(&self, epoch: u32) -> Option<&GroupKey> {
        self.keys.get(&epoch)
    }

    /// How many epochs are retained (reported in `UnknownEpoch`).
    pub fn len(&self) -> usize {
        self.keys.len()
    }

    /// True when no epoch is retained.
    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }
}

/// `version || group || epoch`, little-endian, exactly as it appears in the clear header.
fn aad(version: u16, group: GroupId, epoch: u32) -> [u8; AAD_BYTES] {
    let mut aad = [0u8; AAD_BYTES];
    aad[0..2].copy_from_slice(&version.to_le_bytes());
    aad[2..18].copy_from_slice(&group.0.to_le_bytes());
    aad[18..22].copy_from_slice(&epoch.to_le_bytes());
    aad
}

/// Seals `plaintext` for `group` under `epoch`'s key, prefixing the clear header. A fresh OS nonce
/// per call; two seals of the same bytes never share a ciphertext.
pub fn seal(
    version: u16,
    group: GroupId,
    epoch: u32,
    key: &GroupKey,
    plaintext: &[u8],
) -> Result<Vec<u8>, CryptoError> {
    let mut nonce = [0u8; NONCE_BYTES];
    // Direct OS call, not a passed-in RNG: there is no seam for a seeded PRNG to slip through.
    getrandom::fill(&mut nonce).map_err(|_| CryptoError::Entropy)?;
    let aad = aad(version, group, epoch);
    let ciphertext = key
        .cipher()
        .encrypt(
            XNonce::from_slice(&nonce),
            Payload {
                msg: plaintext,
                aad: &aad,
            },
        )
        .map_err(|_| CryptoError::Encrypt)?;
    let mut out = Vec::with_capacity(SEALED_HEADER_BYTES + ciphertext.len());
    out.extend_from_slice(&aad);
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&ciphertext);
    debug_assert_eq!(out.len(), SEALED_HEADER_BYTES + plaintext.len() + TAG_BYTES);
    Ok(out)
}

/// Opens a sealed batch whose header must match `version` and `group`, using `keys` to find the
/// epoch's key. Every mismatch is its own typed error; the ciphertext is never touched on a header
/// failure.
pub fn open(
    version: u16,
    group: GroupId,
    keys: &GroupKeys,
    sealed: &[u8],
) -> Result<Vec<u8>, CryptoError> {
    let minimum = SEALED_HEADER_BYTES + TAG_BYTES;
    if sealed.len() < minimum {
        return Err(CryptoError::Truncated {
            needed: minimum,
            got: sealed.len(),
        });
    }
    let got_version = u16::from_le_bytes([sealed[0], sealed[1]]);
    if got_version != version {
        return Err(CryptoError::WrongVersion {
            got: got_version,
            expected: version,
        });
    }
    let got_group = read_u128(&sealed[2..18]);
    if got_group != group.0 {
        return Err(CryptoError::WrongGroup {
            got: GroupId(got_group),
            expected: group,
        });
    }
    let epoch = u32::from_le_bytes([sealed[18], sealed[19], sealed[20], sealed[21]]);
    let key = keys.get(epoch).ok_or(CryptoError::UnknownEpoch {
        epoch,
        held: keys.len(),
    })?;
    let aad = sealed[..AAD_BYTES].to_vec();
    let plaintext = key
        .cipher()
        .decrypt(
            XNonce::from_slice(&sealed[AAD_BYTES..SEALED_HEADER_BYTES]),
            Payload {
                msg: &sealed[SEALED_HEADER_BYTES..],
                aad: &aad,
            },
        )
        .map_err(|_| CryptoError::Decrypt { epoch })?;
    debug_assert_eq!(
        plaintext.len() + TAG_BYTES,
        sealed.len() - SEALED_HEADER_BYTES
    );
    Ok(plaintext)
}

/// Reads 16 little-endian bytes without `try_into`, so a length check earlier in `open` is the only
/// precondition and there is no panic path here.
fn read_u128(bytes: &[u8]) -> u128 {
    let mut buf = [0u8; 16];
    buf.copy_from_slice(bytes);
    u128::from_le_bytes(buf)
}
