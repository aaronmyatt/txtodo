//! A stand-in op-signing key for the LAN transport pass (plan M4 `sync-lan-transport`), until real
//! per-device signing keys are distributed through pairing and stored in the devices table (plan
//! M4 `sync-device-remove`) — neither exists yet, so `Session::on_ops`'s per-op signature check
//! (`sync-reject-tests`) has no real `DevicePublicKey` to verify a LAN peer's ops against.
//!
//! **This is not per-device attribution.** [`derive_group_op_signing_key`] derives one Ed25519
//! keypair from the group key via HKDF-SHA256 (info `"txtodo-lan-op-sign-v1"`, distinct from every
//! other label this crate derives — see `sas.rs`/`rotation.rs`), so every device holding the group
//! key derives the *identical* keypair. Verifying against it only proves "the sender holds the
//! group key", exactly what the whole-message AEAD seal (`aead.rs`) already proves — it adds no
//! authentication `seal`/`open` did not already provide. It exists purely so the LAN path can
//! satisfy `on_ops`'s type-level signature requirement without silently skipping it, until real
//! per-device keys make this derivation unnecessary.
//!
//! Ref: <https://docs.rs/hkdf>.

use hkdf::Hkdf;
use sha2::Sha256;

use crate::aead::GroupKey;
use crate::sign::{DeviceSigningKey, SIGNING_KEY_BYTES};

/// HKDF-Expand label for this derivation; must never collide with `sas.rs`'s `SAS_INFO` or
/// `rotation.rs`'s `GRANT_INFO`.
pub const LAN_OP_SIGN_INFO: &[u8] = b"txtodo-lan-op-sign-v1";

/// Derives the stand-in signing keypair described in the module doc from `key`.
pub fn derive_group_op_signing_key(key: &GroupKey) -> DeviceSigningKey {
    let hk = Hkdf::<Sha256>::new(None, key.as_bytes());
    let mut out = [0u8; SIGNING_KEY_BYTES];
    hk.expand(LAN_OP_SIGN_INFO, &mut out)
        .unwrap_or_else(|_| unreachable!("32 bytes is within HKDF-SHA256's output limit"));
    DeviceSigningKey::from_bytes(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derivation_is_deterministic_from_the_same_group_key() {
        let key = GroupKey::from_bytes([7u8; crate::aead::KEY_BYTES]);
        let a = derive_group_op_signing_key(&key).public_key().to_bytes();
        let b = derive_group_op_signing_key(&key).public_key().to_bytes();
        assert_eq!(
            a, b,
            "both sides of a paired connection must derive the same keypair"
        );
    }

    #[test]
    fn different_group_keys_derive_different_keypairs() {
        let a = GroupKey::from_bytes([1u8; crate::aead::KEY_BYTES]);
        let b = GroupKey::from_bytes([2u8; crate::aead::KEY_BYTES]);
        assert_ne!(
            derive_group_op_signing_key(&a).public_key().to_bytes(),
            derive_group_op_signing_key(&b).public_key().to_bytes()
        );
    }
}
