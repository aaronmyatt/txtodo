//! The bundle's own Argon2id -> XChaCha20-Poly1305 wrap (plan M8 `cli-bundle`, design §4.5): same
//! RFC 9106 KDF and AEAD as `txtodo_sync::FileKeyStore`
//! (`crates/txtodo-sync/src/keystore_file.rs`), a different wire shape because a bundle is a
//! *stream* — the STREAM construction seals one bounded chunk at a time — where the keystore file
//! is one small blob sealed whole.
//! Refs: <https://docs.rs/argon2> · RFC 9106 <https://www.rfc-editor.org/rfc/rfc9106> ·
//! <https://docs.rs/chacha20poly1305> · <https://docs.rs/aead/latest/aead/stream/index.html>

use argon2::{Algorithm, Argon2, Params, Version};
use chacha20poly1305::aead::Payload;
use chacha20poly1305::aead::stream::{DecryptorBE32, EncryptorBE32};
use chacha20poly1305::{KeyInit, XChaCha20Poly1305};
use txtodo_sync::{ARGON2_ITERATIONS, ARGON2_MEMORY_KIB, ARGON2_PARALLELISM};

/// Bundle header magic, distinct from `txtodo_sync::keystore_file`'s `TXKS` (a different format).
const MAGIC: &[u8; 4] = b"TXBN";
/// Bundle wire/header format version — bumped independently of `BundleManifest.version` (the
/// header format and the manifest schema can each change on their own).
const HEADER_FORMAT_VERSION: u16 = 1;
const SALT_BYTES: usize = 16;
/// XChaCha20-Poly1305's 24-byte nonce, minus the STREAM construction's 4-byte counter and 1-byte
/// last-block flag (`aead::stream`'s `StreamBE32` scheme) — the fixed part every chunk shares.
const NONCE_PREFIX_BYTES: usize = 19;
const KEY_BYTES: usize = 32;
/// Header length: magic + version + three Argon2 params + salt + nonce prefix. Sent in the clear
/// as the bundle's very first frame, and doubles as the AEAD associated data for every sealed
/// chunk, so a header field cannot be swapped after the fact without failing every chunk's tag.
pub(crate) const HEADER_BYTES: usize = 4 + 2 + 4 + 4 + 4 + SALT_BYTES + NONCE_PREFIX_BYTES;
/// Plaintext bytes per sealed chunk before wrapping (design §4.5's "bounded frame (64 KiB)").
pub(crate) const CHUNK_PLAINTEXT_MAX: usize = 64 * 1024;

/// The clear header: Argon2 parameters and salt (so a bundle written under today's constants
/// still opens if they're raised later, same rule as `FileKeyStore`) plus the STREAM nonce prefix.
#[derive(Clone, Copy)]
pub(crate) struct BundleHeader {
    memory_kib: u32,
    iterations: u32,
    parallelism: u32,
    salt: [u8; SALT_BYTES],
    nonce_prefix: [u8; NONCE_PREFIX_BYTES],
}

/// Why the header or the key derived from it could not be used. Each variant is its own
/// import-visible stage (`bundle_import.rs`'s `BundleImportError` wraps these) — a wrong
/// passphrase and a tampered ciphertext chunk both surface as `Aead` (that is exactly what an AEAD
/// tag proves: "the wrong key or the wrong bytes", never which); a corrupt clear header is
/// reported before either is even attempted.
#[derive(Debug)]
pub(crate) enum BundleCryptoError {
    /// Fewer than [`HEADER_BYTES`] bytes.
    Truncated,
    /// The first four bytes are not [`MAGIC`].
    BadMagic,
    /// A header version this build does not speak.
    UnsupportedVersion(u16),
    /// The stored Argon2 parameters are not valid ones.
    BadParams(String),
    /// The Argon2 KDF itself failed.
    Kdf(String),
    /// The OS entropy source failed while minting a fresh header.
    Entropy,
    /// An AEAD chunk failed to open, or failed to seal (should not happen with a live key).
    Aead,
}

impl std::fmt::Display for BundleCryptoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BundleCryptoError::Truncated => write!(f, "bundle header is truncated"),
            BundleCryptoError::BadMagic => write!(f, "not a txtodo bundle (bad magic)"),
            BundleCryptoError::UnsupportedVersion(v) => {
                write!(f, "bundle header version {v} is not supported")
            }
            BundleCryptoError::BadParams(e) => write!(f, "invalid Argon2 parameters: {e}"),
            BundleCryptoError::Kdf(e) => write!(f, "Argon2 key derivation failed: {e}"),
            BundleCryptoError::Entropy => write!(f, "entropy source failed"),
            BundleCryptoError::Aead => write!(f, "wrong passphrase, or the bundle is corrupt"),
        }
    }
}

impl std::error::Error for BundleCryptoError {}

fn random_bytes<const N: usize>() -> Result<[u8; N], BundleCryptoError> {
    let mut buf = [0u8; N];
    getrandom::fill(&mut buf).map_err(|_| BundleCryptoError::Entropy)?;
    Ok(buf)
}

impl BundleHeader {
    /// A fresh header for a new export: today's Argon2 constants, a random salt and nonce prefix.
    pub(crate) fn fresh() -> Result<BundleHeader, BundleCryptoError> {
        Ok(BundleHeader {
            memory_kib: ARGON2_MEMORY_KIB,
            iterations: ARGON2_ITERATIONS,
            parallelism: ARGON2_PARALLELISM,
            salt: random_bytes()?,
            nonce_prefix: random_bytes()?,
        })
    }

    /// Encodes the header — also this bundle's AEAD associated data.
    pub(crate) fn encode(&self) -> [u8; HEADER_BYTES] {
        let mut out = [0u8; HEADER_BYTES];
        out[0..4].copy_from_slice(MAGIC);
        out[4..6].copy_from_slice(&HEADER_FORMAT_VERSION.to_le_bytes());
        out[6..10].copy_from_slice(&self.memory_kib.to_le_bytes());
        out[10..14].copy_from_slice(&self.iterations.to_le_bytes());
        out[14..18].copy_from_slice(&self.parallelism.to_le_bytes());
        out[18..18 + SALT_BYTES].copy_from_slice(&self.salt);
        out[18 + SALT_BYTES..HEADER_BYTES].copy_from_slice(&self.nonce_prefix);
        out
    }

    /// Decodes a header — refuses a bad magic or an unsupported version before anything else
    /// (design §4.5's "reject before allocating"), well before the Argon2 KDF ever runs.
    pub(crate) fn decode(bytes: &[u8]) -> Result<BundleHeader, BundleCryptoError> {
        if bytes.len() < HEADER_BYTES {
            return Err(BundleCryptoError::Truncated);
        }
        if &bytes[0..4] != MAGIC {
            return Err(BundleCryptoError::BadMagic);
        }
        let version = u16::from_le_bytes([bytes[4], bytes[5]]);
        if version != HEADER_FORMAT_VERSION {
            return Err(BundleCryptoError::UnsupportedVersion(version));
        }
        let memory_kib = u32::from_le_bytes([bytes[6], bytes[7], bytes[8], bytes[9]]);
        let iterations = u32::from_le_bytes([bytes[10], bytes[11], bytes[12], bytes[13]]);
        let parallelism = u32::from_le_bytes([bytes[14], bytes[15], bytes[16], bytes[17]]);
        let mut salt = [0u8; SALT_BYTES];
        salt.copy_from_slice(&bytes[18..18 + SALT_BYTES]);
        let mut nonce_prefix = [0u8; NONCE_PREFIX_BYTES];
        nonce_prefix.copy_from_slice(&bytes[18 + SALT_BYTES..HEADER_BYTES]);
        Ok(BundleHeader {
            memory_kib,
            iterations,
            parallelism,
            salt,
            nonce_prefix,
        })
    }

    fn derive_key(&self, passphrase: &[u8]) -> Result<[u8; KEY_BYTES], BundleCryptoError> {
        let params = Params::new(
            self.memory_kib,
            self.iterations,
            self.parallelism,
            Some(KEY_BYTES),
        )
        .map_err(|e| BundleCryptoError::BadParams(e.to_string()))?;
        let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
        let mut key = [0u8; KEY_BYTES];
        argon2
            .hash_password_into(passphrase, &self.salt, &mut key)
            .map_err(|e| BundleCryptoError::Kdf(e.to_string()))?;
        Ok(key)
    }
}

/// Seals successive plaintext chunks under one bundle's key/header (STREAM construction: each
/// chunk gets a fresh, non-repeating nonce derived from the header's prefix plus a monotonic
/// counter — never a manually chosen nonce).
pub(crate) struct BundleEncryptor {
    inner: EncryptorBE32<XChaCha20Poly1305>,
    aad: [u8; HEADER_BYTES],
}

impl BundleEncryptor {
    pub(crate) fn new(
        header: &BundleHeader,
        passphrase: &[u8],
    ) -> Result<BundleEncryptor, BundleCryptoError> {
        let key = header.derive_key(passphrase)?;
        let cipher = XChaCha20Poly1305::new((&key).into());
        Ok(BundleEncryptor {
            inner: EncryptorBE32::from_aead(cipher, (&header.nonce_prefix).into()),
            aad: header.encode(),
        })
    }

    /// Seals one non-final chunk; `plaintext.len()` must be at most [`CHUNK_PLAINTEXT_MAX`].
    pub(crate) fn encrypt_next(&mut self, plaintext: &[u8]) -> Result<Vec<u8>, BundleCryptoError> {
        debug_assert!(plaintext.len() <= CHUNK_PLAINTEXT_MAX);
        self.inner
            .encrypt_next(Payload {
                msg: plaintext,
                aad: &self.aad,
            })
            .map_err(|_| BundleCryptoError::Aead)
    }

    /// Seals the final chunk (may be empty); consumes the encryptor — the STREAM construction has
    /// no "seal another chunk after this" call by design.
    pub(crate) fn encrypt_last(self, plaintext: &[u8]) -> Result<Vec<u8>, BundleCryptoError> {
        self.inner
            .encrypt_last(Payload {
                msg: plaintext,
                aad: &self.aad,
            })
            .map_err(|_| BundleCryptoError::Aead)
    }
}

/// Opens chunks sealed by [`BundleEncryptor`], in the same order.
pub(crate) struct BundleDecryptor {
    inner: DecryptorBE32<XChaCha20Poly1305>,
    aad: [u8; HEADER_BYTES],
}

impl BundleDecryptor {
    pub(crate) fn new(
        header: &BundleHeader,
        passphrase: &[u8],
    ) -> Result<BundleDecryptor, BundleCryptoError> {
        let key = header.derive_key(passphrase)?;
        let cipher = XChaCha20Poly1305::new((&key).into());
        Ok(BundleDecryptor {
            inner: DecryptorBE32::from_aead(cipher, (&header.nonce_prefix).into()),
            aad: header.encode(),
        })
    }

    /// Opens one non-final chunk. A wrong passphrase and a tampered chunk are indistinguishable
    /// here by construction — see [`BundleCryptoError::Aead`]'s own doc.
    pub(crate) fn decrypt_next(&mut self, chunk: &[u8]) -> Result<Vec<u8>, BundleCryptoError> {
        self.inner
            .decrypt_next(Payload {
                msg: chunk,
                aad: &self.aad,
            })
            .map_err(|_| BundleCryptoError::Aead)
    }

    /// Opens the final chunk; consumes the decryptor.
    pub(crate) fn decrypt_last(self, chunk: &[u8]) -> Result<Vec<u8>, BundleCryptoError> {
        self.inner
            .decrypt_last(Payload {
                msg: chunk,
                aad: &self.aad,
            })
            .map_err(|_| BundleCryptoError::Aead)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_round_trips_and_rejects_bad_magic_and_version() {
        let header = BundleHeader::fresh().unwrap();
        let bytes = header.encode();
        let back = BundleHeader::decode(&bytes).unwrap();
        assert_eq!(back.salt, header.salt);
        assert_eq!(back.nonce_prefix, header.nonce_prefix);
        let mut bad_magic = bytes;
        bad_magic[0] ^= 0xFF;
        assert!(matches!(
            BundleHeader::decode(&bad_magic),
            Err(BundleCryptoError::BadMagic)
        ));
        let mut bad_version = bytes;
        bad_version[4] ^= 0xFF;
        assert!(matches!(
            BundleHeader::decode(&bad_version),
            Err(BundleCryptoError::UnsupportedVersion(_))
        ));
        assert!(matches!(
            BundleHeader::decode(&bytes[..HEADER_BYTES - 1]),
            Err(BundleCryptoError::Truncated)
        ));
    }

    #[test]
    fn encrypt_then_decrypt_round_trips_multiple_chunks() {
        let header = BundleHeader::fresh().unwrap();
        let mut enc = BundleEncryptor::new(&header, b"correct horse battery staple").unwrap();
        let c0 = enc.encrypt_next(b"hello ").unwrap();
        let c1 = enc.encrypt_last(b"world").unwrap();
        let mut dec = BundleDecryptor::new(&header, b"correct horse battery staple").unwrap();
        let p0 = dec.decrypt_next(&c0).unwrap();
        let p1 = dec.decrypt_last(&c1).unwrap();
        assert_eq!(&p0, b"hello ");
        assert_eq!(&p1, b"world");
    }

    #[test]
    fn wrong_passphrase_fails_to_open_the_first_chunk() {
        let header = BundleHeader::fresh().unwrap();
        let enc = BundleEncryptor::new(&header, b"right").unwrap();
        let c0 = enc.encrypt_last(b"secret").unwrap();
        let mut dec = BundleDecryptor::new(&header, b"wrong").unwrap();
        assert!(matches!(
            dec.decrypt_next(&c0),
            Err(BundleCryptoError::Aead)
        ));
    }

    #[test]
    fn a_flipped_ciphertext_byte_fails_the_aead_tag() {
        let header = BundleHeader::fresh().unwrap();
        let enc = BundleEncryptor::new(&header, b"passphrase").unwrap();
        let mut c0 = enc.encrypt_last(b"payload").unwrap();
        let last = c0.len() - 1;
        c0[last] ^= 0x01;
        let mut dec = BundleDecryptor::new(&header, b"passphrase").unwrap();
        assert!(matches!(
            dec.decrypt_next(&c0),
            Err(BundleCryptoError::Aead)
        ));
    }
}
