//! One typed error for every crypto path. Nothing here panics and nothing here is a normal
//! condition: signatures and AEAD tags come from another device, so every failure is validated on
//! the way in (CLAUDE.md §3). Each variant names what failed and against which epoch, device or
//! group, so a log line says which key to look at instead of "decryption failed".

use std::fmt;

use txtodo_model::DeviceId;

use crate::message::GroupId;

/// Why a signature or a sealed batch was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CryptoError {
    /// `Op::signing_bytes` could not encode the op (postcard errored).
    Encode(postcard::Error),
    /// A batch's op and signature counts differ; never verified partially.
    BatchLength {
        /// Ops supplied.
        ops: usize,
        /// Signatures supplied.
        signatures: usize,
    },
    /// No verifying key is known for the op's origin device.
    UnknownDevice {
        /// The signer the batch claimed.
        device: DeviceId,
    },
    /// A public key was not a valid Ed25519 point.
    BadPublicKey {
        /// The device the key was labelled with, when the caller knows it.
        device: Option<DeviceId>,
    },
    /// Ed25519 verification failed for an op from `device`.
    SignatureInvalid {
        /// The origin device whose signature did not verify.
        device: DeviceId,
    },
    /// The sealed batch's protocol version is not the one we speak.
    WrongVersion {
        /// Version found in the clear header.
        got: u16,
        /// Version this build expects.
        expected: u16,
    },
    /// The sealed batch names another group's key material.
    WrongGroup {
        /// Group found in the clear header.
        got: GroupId,
        /// Group this session is for.
        expected: GroupId,
    },
    /// The sealed blob is shorter than a header plus an AEAD tag.
    Truncated {
        /// Smallest acceptable length.
        needed: usize,
        /// Bytes actually supplied.
        got: usize,
    },
    /// No key is retained for the epoch named in the clear header. Never a try-every-key loop.
    UnknownEpoch {
        /// The epoch from the header.
        epoch: u32,
        /// Epochs currently retained, so the message says how much history is left.
        held: usize,
    },
    /// Retaining another epoch would exceed `MAX_RETAINED_KEY_EPOCHS`.
    TooManyEpochs {
        /// Epochs after the insert would be attempted.
        len: usize,
        /// The cap.
        max: usize,
    },
    /// XChaCha20-Poly1305 refused to seal (plaintext length is the only cause).
    Encrypt,
    /// The AEAD tag did not verify: wrong key, tampered ciphertext, or a foreign group/version.
    Decrypt {
        /// The epoch the failed decrypt was attempted under.
        epoch: u32,
    },
    /// The OS CSPRNG could not produce a nonce. Fatal, and never retried with a weaker source.
    Entropy,
}

impl fmt::Display for CryptoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CryptoError::Encode(e) => write!(f, "cannot encode op for signing: {e}"),
            CryptoError::BatchLength { ops, signatures } => write!(
                f,
                "batch has {ops} ops but {signatures} signatures; refusing to verify partially"
            ),
            CryptoError::UnknownDevice { device } => {
                write!(f, "no verifying key held for device {device}")
            }
            CryptoError::BadPublicKey { device: Some(d) } => {
                write!(f, "public key for device {d} is not a valid Ed25519 point")
            }
            CryptoError::BadPublicKey { device: None } => {
                write!(f, "public key is not a valid Ed25519 point")
            }
            CryptoError::SignatureInvalid { device } => {
                write!(f, "signature from device {device} did not verify")
            }
            CryptoError::WrongVersion { got, expected } => {
                write!(f, "sealed batch speaks version {got}, expected {expected}")
            }
            CryptoError::WrongGroup { got, expected } => {
                write!(
                    f,
                    "sealed batch is for group {}, expected {}",
                    got.0, expected.0
                )
            }
            CryptoError::Truncated { needed, got } => {
                write!(f, "sealed batch needs {needed} bytes, only {got} available")
            }
            CryptoError::UnknownEpoch { epoch, held } => {
                write!(f, "no group key retained for epoch {epoch} ({held} held)")
            }
            CryptoError::TooManyEpochs { len, max } => {
                write!(
                    f,
                    "retaining {len} group-key epochs exceeds the cap of {max}"
                )
            }
            CryptoError::Encrypt => write!(f, "XChaCha20-Poly1305 refused to seal the batch"),
            CryptoError::Decrypt { epoch } => {
                write!(
                    f,
                    "AEAD tag failed for epoch {epoch}: wrong key or tampered"
                )
            }
            CryptoError::Entropy => write!(f, "the OS CSPRNG failed; no nonce was produced"),
        }
    }
}

impl std::error::Error for CryptoError {}

impl From<postcard::Error> for CryptoError {
    fn from(e: postcard::Error) -> CryptoError {
        CryptoError::Encode(e)
    }
}
