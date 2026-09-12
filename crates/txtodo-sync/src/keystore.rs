//! Where every key lives: the device Ed25519 signing key ([`crate::sign`]), the device's long-term
//! X25519 static key (M4 `sync-device-remove`), and the group key per epoch ([`crate::aead`]). Never
//! reads or writes any of them itself — that is `keystore_memory`, `keystore_file`, `keystore_os`
//! and `keystore_resolve`; this module is only the shape every backend agrees on.
//! Ref: <https://docs.rs/keyring>.

use std::fmt;

use serde::{Deserialize, Serialize};
use zeroize::Zeroize;

use crate::aead::MAX_RETAINED_KEY_EPOCHS;
use crate::keystore_error::KeyStoreError;

/// Group-key epochs a keystore backend will retain, mirroring [`MAX_RETAINED_KEY_EPOCHS`]. Defined
/// once and imported so the two caps cannot drift apart.
pub const MAX_STORED_EPOCHS: usize = MAX_RETAINED_KEY_EPOCHS;

/// Which secret is being asked for. An enum, not a string key, so a typo cannot silently create a
/// second key under a slightly different name.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum KeyId {
    /// This device's Ed25519 op-signing key.
    DeviceSigning,
    /// This device's long-term X25519 static key, registered at pairing so other devices can wrap
    /// a rotated group key to it later.
    DeviceStatic,
    /// The group key for one epoch.
    Group(u32),
}

impl fmt::Display for KeyId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            KeyId::DeviceSigning => write!(f, "device-signing"),
            KeyId::DeviceStatic => write!(f, "device-static"),
            KeyId::Group(epoch) => write!(f, "group-epoch-{epoch}"),
        }
    }
}

/// Raw key bytes. Wiped on drop; never `Debug`/`Display`s its contents — a derived `Debug` on a key
/// type is how keys end up in logs and `tracing` spans (CLAUDE.md §3.1).
pub struct Secret(Vec<u8>);

impl Secret {
    /// Takes ownership of `bytes`; the caller's copy should be dropped (or was already moved).
    pub fn new(bytes: Vec<u8>) -> Secret {
        Secret(bytes)
    }

    /// The raw bytes, for the one call site that needs to construct a key type from them.
    pub fn expose(&self) -> &[u8] {
        &self.0
    }
}

impl Clone for Secret {
    fn clone(&self) -> Secret {
        Secret(self.0.clone())
    }
}

impl PartialEq for Secret {
    fn eq(&self, other: &Secret) -> bool {
        self.0 == other.0
    }
}

impl Eq for Secret {}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(<redacted>)")
    }
}

impl Drop for Secret {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

/// A place keys are read from and written to. `get`/`put`/`delete` are the whole surface: callers
/// never see a backend-specific type, and every backend is exercised by the same test suite.
pub trait KeyStore {
    /// The secret for `id`, or `None` if nothing has been stored under it yet.
    fn get(&self, id: KeyId) -> Result<Option<Secret>, KeyStoreError>;
    /// Stores (or replaces) the secret for `id`.
    fn put(&self, id: KeyId, secret: &Secret) -> Result<(), KeyStoreError>;
    /// Removes the secret for `id`, if present. Removing an absent id is not an error.
    fn delete(&self, id: KeyId) -> Result<(), KeyStoreError>;
}
