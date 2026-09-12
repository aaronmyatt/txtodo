//! Wraps a rotated group key to each remaining device's long-term static public key (plan M4
//! `sync-device-remove`), so a device offline at rotation time still finds its grant when it
//! returns. One ephemeral ECDH per recipient: `wrap_grant_for` generates a fresh ephemeral
//! keypair, derives a wrap key via HKDF-SHA256 over the ECDH output salted with both public keys,
//! and seals the new group key under XChaCha20-Poly1305 — the recipient's long-term secret never
//! doubles as an AEAD key directly. `open_grant` redoes the same ECDH with the recipient's
//! long-term secret. Refs: <https://docs.rs/x25519-dalek> · <https://docs.rs/hkdf>.
//!
//! This module only seals/opens/plans; it never decides *when* to rotate, never touches a
//! `devices` table, and never enforces the "close the epoch before announcing the removal"
//! ordering — those are daemon-level sequencing concerns over data this crate does not hold.

use std::collections::BTreeMap;

use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use hkdf::Hkdf;
use rand_core::OsRng;
use sha2::Sha256;
use txtodo_model::DeviceId;
use x25519_dalek::{EphemeralSecret, PublicKey};

use crate::aead::KEY_BYTES;
use crate::device_static::{DEVICE_STATIC_KEY_BYTES, DeviceStaticPublic, DeviceStaticSecret};
use crate::rotation_error::{RemovalError, RotationError};

/// `HKDF-Expand` label for a rotation grant's wrap key. Distinct from the pairing labels
/// (`SAS_INFO`/`PAIR_KEY_INFO`) — never the same derived bytes reused for two purposes.
pub const GRANT_INFO: &[u8] = b"txtodo-rotation-grant-v1";
const NONCE_BYTES: usize = 24;

/// One epoch's group key, sealed to one remaining device's long-term static public key.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WrappedGrant {
    /// The group-key epoch this grant carries.
    pub epoch: u32,
    /// The one-time ephemeral public key the recipient needs to redo the ECDH.
    pub ephemeral_public: [u8; DEVICE_STATIC_KEY_BYTES],
    /// `nonce || ciphertext`.
    pub sealed: Vec<u8>,
}

fn derive_wrap_key(
    shared: &[u8; DEVICE_STATIC_KEY_BYTES],
    ephemeral_public: &[u8; DEVICE_STATIC_KEY_BYTES],
    recipient_public: &[u8; DEVICE_STATIC_KEY_BYTES],
) -> Result<[u8; KEY_BYTES], RotationError> {
    let mut salt = [0u8; DEVICE_STATIC_KEY_BYTES * 2];
    salt[..DEVICE_STATIC_KEY_BYTES].copy_from_slice(ephemeral_public);
    salt[DEVICE_STATIC_KEY_BYTES..].copy_from_slice(recipient_public);
    let hk = Hkdf::<Sha256>::new(Some(&salt), shared);
    let mut out = [0u8; KEY_BYTES];
    hk.expand(GRANT_INFO, &mut out)
        .map_err(|_| RotationError::Derive)?;
    Ok(out)
}

/// Seals `new_key_bytes` (a fresh group key's raw 32 bytes) for `recipient` under `epoch`.
pub fn wrap_grant_for(
    new_key_bytes: &[u8; KEY_BYTES],
    epoch: u32,
    recipient: &DeviceStaticPublic,
) -> Result<WrappedGrant, RotationError> {
    let eph_secret = EphemeralSecret::random_from_rng(OsRng);
    let eph_public = PublicKey::from(&eph_secret).to_bytes();
    let shared = eph_secret
        .diffie_hellman(&PublicKey::from(recipient.to_bytes()))
        .to_bytes();
    let key = derive_wrap_key(&shared, &eph_public, &recipient.to_bytes())?;
    let mut nonce = [0u8; NONCE_BYTES];
    getrandom::fill(&mut nonce).map_err(|_| RotationError::Entropy)?;
    let ciphertext = XChaCha20Poly1305::new((&key).into())
        .encrypt(
            XNonce::from_slice(&nonce),
            Payload {
                msg: new_key_bytes,
                aad: &epoch.to_le_bytes(),
            },
        )
        .map_err(|_| RotationError::Seal)?;
    let mut sealed = Vec::with_capacity(NONCE_BYTES + ciphertext.len());
    sealed.extend_from_slice(&nonce);
    sealed.extend_from_slice(&ciphertext);
    Ok(WrappedGrant {
        epoch,
        ephemeral_public: eph_public,
        sealed,
    })
}

/// Opens a grant sealed by [`wrap_grant_for`], recovering the group key's raw bytes.
pub fn open_grant(
    grant: &WrappedGrant,
    my_secret: &DeviceStaticSecret,
) -> Result<[u8; KEY_BYTES], RotationError> {
    if grant.sealed.len() < NONCE_BYTES {
        return Err(RotationError::Open);
    }
    let shared = my_secret.diffie_hellman_with(&grant.ephemeral_public);
    let my_public = my_secret.public_key().to_bytes();
    let key = derive_wrap_key(&shared, &grant.ephemeral_public, &my_public)?;
    let (nonce, ciphertext) = grant.sealed.split_at(NONCE_BYTES);
    let plaintext = XChaCha20Poly1305::new((&key).into())
        .decrypt(
            XNonce::from_slice(nonce),
            Payload {
                msg: ciphertext,
                aad: &grant.epoch.to_le_bytes(),
            },
        )
        .map_err(|_| RotationError::Open)?;
    plaintext.try_into().map_err(|_| RotationError::Open)
}

/// Refuses a removal before any crypto runs: never yourself, never the last device.
pub fn validate_removal(
    removing: DeviceId,
    this_device: DeviceId,
    devices_before: usize,
) -> Result<(), RemovalError> {
    if removing == this_device {
        return Err(RemovalError::CannotRemoveSelf { device: removing });
    }
    if devices_before <= 1 {
        return Err(RemovalError::CannotRemoveLastDevice);
    }
    Ok(())
}

/// Plans a rotation to `current_epoch + 1`: one [`WrappedGrant`] per entry in `remaining`, the
/// devices that keep the group after the removal. The caller supplies the fresh key bytes (drawn
/// from the OS CSPRNG, same discipline as every other key in this crate) and separately inserts
/// them into its own `GroupKeys` under the new epoch — this function only produces what gets sent
/// to *other* devices.
pub fn plan_rotation(
    current_epoch: u32,
    new_key_bytes: &[u8; KEY_BYTES],
    remaining: &BTreeMap<DeviceId, DeviceStaticPublic>,
) -> Result<BTreeMap<DeviceId, WrappedGrant>, RotationError> {
    if remaining.is_empty() {
        return Err(RotationError::NoRemainingDevices);
    }
    let new_epoch = current_epoch
        .checked_add(1)
        .ok_or(RotationError::EpochOverflow)?;
    let mut grants = BTreeMap::new();
    for (device, public) in remaining {
        grants.insert(*device, wrap_grant_for(new_key_bytes, new_epoch, public)?);
    }
    debug_assert_eq!(grants.len(), remaining.len());
    Ok(grants)
}
