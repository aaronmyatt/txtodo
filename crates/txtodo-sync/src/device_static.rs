//! Each device's long-term X25519 keypair, registered once at pairing (plan M4
//! `sync-device-remove`) so a remaining device can later be handed a rotated group key without
//! needing to be online at the moment of rotation. Distinct from the *ephemeral* per-handshake key
//! in `pairing.rs`: this one is generated once, stored via the keystore under
//! `KeyId::DeviceStatic`, and never thrown away. Ref: <https://docs.rs/x25519-dalek>.

use std::fmt;

use x25519_dalek::{PublicKey, StaticSecret};

/// Bytes in an X25519 static key, public or private.
pub const DEVICE_STATIC_KEY_BYTES: usize = 32;

/// A device's long-term X25519 private key. Opaque and redacted like every other key type here —
/// `x25519-dalek`'s own `zeroize` feature (on by default) wipes it on drop.
#[derive(Clone)]
pub struct DeviceStaticSecret(StaticSecret);

impl DeviceStaticSecret {
    /// Wraps raw bytes read back from the keystore.
    pub fn from_bytes(bytes: [u8; DEVICE_STATIC_KEY_BYTES]) -> DeviceStaticSecret {
        DeviceStaticSecret(StaticSecret::from(bytes))
    }

    /// Generates a fresh key straight from the OS CSPRNG, for registration at pairing.
    pub fn generate() -> DeviceStaticSecret {
        DeviceStaticSecret(StaticSecret::random())
    }

    /// Raw bytes, to persist via the keystore. Never logged; the caller owns that discipline.
    pub fn to_bytes(&self) -> [u8; DEVICE_STATIC_KEY_BYTES] {
        self.0.to_bytes()
    }

    /// The matching public key, safe to ship to peers and store alongside a `DeviceId`.
    pub fn public_key(&self) -> DeviceStaticPublic {
        DeviceStaticPublic(PublicKey::from(&self.0).to_bytes())
    }

    /// The X25519 shared secret with an ephemeral public key from a rotation grant. Returns plain
    /// bytes, not a `SharedSecret`, so `rotation.rs` never needs to name an `x25519_dalek` type.
    pub fn diffie_hellman_with(
        &self,
        ephemeral_public: &[u8; DEVICE_STATIC_KEY_BYTES],
    ) -> [u8; DEVICE_STATIC_KEY_BYTES] {
        *self
            .0
            .diffie_hellman(&PublicKey::from(*ephemeral_public))
            .as_bytes()
    }
}

impl fmt::Debug for DeviceStaticSecret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("DeviceStaticSecret(<redacted>)")
    }
}

/// A device's long-term X25519 public key, learned at pairing and stored per remaining device so
/// a rotation can wrap a new group key to it even while that device is offline. Not secret.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct DeviceStaticPublic([u8; DEVICE_STATIC_KEY_BYTES]);

impl DeviceStaticPublic {
    /// Wraps raw bytes carried over the wire (e.g. in a pairing confirmation payload).
    pub fn from_bytes(bytes: [u8; DEVICE_STATIC_KEY_BYTES]) -> DeviceStaticPublic {
        DeviceStaticPublic(bytes)
    }

    /// Raw bytes, to send to a peer or store in the `devices` table.
    pub fn to_bytes(&self) -> [u8; DEVICE_STATIC_KEY_BYTES] {
        self.0
    }
}
