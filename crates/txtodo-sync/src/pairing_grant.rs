//! What actually moves through [`crate::pairing::PairingSession::wrap_group_key`] /
//! `unwrap_group_key` once both sides confirm: the group key **and** the sender's long-term
//! static X25519 public key, bundled into one payload. Plan M4 `sync-device-remove`'s own notes
//! say why the second half matters: "the static key has to be registered [at pairing], so the two
//! tasks land in that order or rotation has nothing to wrap to" — bundling it here means a future
//! caller cannot send the group key while forgetting the registration.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::aead::KEY_BYTES;
use crate::device_static::DEVICE_STATIC_KEY_BYTES;

/// The group key plus the sender's static public key, as carried inside one pairing confirmation.
/// Hand-written `Debug`: a derived one would print `group_key`'s raw bytes, which is exactly the
/// key-in-a-log-line mistake `KeyId`/`Secret`/`GroupKey` are already careful to avoid.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PairingGrant {
    /// The current group key's raw bytes.
    pub group_key: [u8; KEY_BYTES],
    /// The sender's long-term X25519 public key, for `sync-device-remove` to wrap future
    /// rotations to. Not secret, but printed as hex rather than a raw byte-array Debug for
    /// consistency with every other key type here.
    pub static_public: [u8; DEVICE_STATIC_KEY_BYTES],
}

impl fmt::Debug for PairingGrant {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PairingGrant")
            .field("group_key", &"<redacted>")
            .field("static_public", &hex(&self.static_public))
            .finish()
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Why a `PairingGrant` could not be encoded or decoded.
#[derive(Debug)]
pub enum PairingGrantError {
    /// Could not serialize the grant to bytes.
    Encode(postcard::Error),
    /// The bytes were not a valid grant.
    Decode(postcard::Error),
}

impl std::fmt::Display for PairingGrantError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PairingGrantError::Encode(e) => write!(f, "cannot encode pairing grant: {e}"),
            PairingGrantError::Decode(e) => write!(f, "cannot decode pairing grant: {e}"),
        }
    }
}

impl std::error::Error for PairingGrantError {}

impl PairingGrant {
    /// Encodes for `PairingSession::wrap_group_key`'s plaintext argument.
    pub fn to_bytes(&self) -> Result<Vec<u8>, PairingGrantError> {
        postcard::to_allocvec(self).map_err(PairingGrantError::Encode)
    }

    /// Decodes what `PairingSession::unwrap_group_key` returned.
    pub fn from_bytes(bytes: &[u8]) -> Result<PairingGrant, PairingGrantError> {
        postcard::from_bytes(bytes).map_err(PairingGrantError::Decode)
    }
}
