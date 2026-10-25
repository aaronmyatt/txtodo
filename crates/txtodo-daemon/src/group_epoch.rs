//! The group key at the epochs a device holds it (task `peer-prune`, ADR 0044). A rotation
//! (`device remove`, and so a prune) makes a new key at the next epoch; a device seals what it sends
//! under its current epoch and opens what it receives under any epoch it still retains, so a frame
//! sealed just before a rotation, or one a slower peer sends, still opens. Every sync path (the live
//! sessions, the file carrier, the control channel) reads its keys here, so none of them knows the
//! epoch is a number that moves.
//!
//! The op signatures ride on the key too (`derive_group_op_signing_key`: a stand-in until real
//! per-device keys), so a batch is verified with the key of the epoch it was sealed under, never the
//! current one.

use txtodo_sync::{
    DevicePublicKey, DeviceSigningKey, GroupKey, GroupKeys, KEY_BYTES, KeyId, KeyStore,
    MAX_RETAINED_KEY_EPOCHS, derive_group_op_signing_key,
};

/// A group key with the epoch it belongs to.
#[derive(Clone)]
pub(crate) struct EpochKey {
    /// The generation this key opens and seals.
    pub(crate) epoch: u32,
    /// The key itself; never printed.
    pub(crate) key: GroupKey,
}

impl EpochKey {
    /// The key batches sent now are signed with.
    pub(crate) fn signing_key(&self) -> DeviceSigningKey {
        derive_group_op_signing_key(&self.key)
    }
}

/// `epoch`'s key bytes from `store`, for a caller that wraps them to someone (`group_rotation.rs`).
/// `Ok(None)` when none is stored, `Err` (what a human should hear) when the keystore fails or the
/// stored bytes are not a key.
pub(crate) fn fetch_raw(
    store: &dyn KeyStore,
    epoch: u32,
) -> Result<Option<[u8; KEY_BYTES]>, String> {
    let stored = store
        .get(KeyId::Group(epoch))
        .map_err(|e| format!("the keystore could not read the group key: {e}"))?;
    let Some(bytes) = stored else {
        return Ok(None);
    };
    let raw = bytes.expose();
    let array = raw
        .try_into()
        .map_err(|_| format!("the stored group key is {} bytes, not 32", raw.len()))?;
    Ok(Some(array))
}

/// [`fetch_raw`] as a [`GroupKey`].
pub(crate) fn fetch(store: &dyn KeyStore, epoch: u32) -> Result<Option<GroupKey>, String> {
    Ok(fetch_raw(store, epoch)?.map(GroupKey::from_bytes))
}

/// Every epoch up to `current` that `store` holds, the newest [`MAX_RETAINED_KEY_EPOCHS`] of them.
/// `None` when the current epoch itself is not held (nothing to seal under) or the table refuses it.
/// An older epoch that is missing or unreadable is left out: its frames then fail to open, which
/// is what they would do anyway.
pub(crate) fn retained(store: &dyn KeyStore, current: u32) -> Option<GroupKeys> {
    let oldest = current.saturating_sub(u32::try_from(MAX_RETAINED_KEY_EPOCHS - 1).unwrap_or(0));
    let mut keys = GroupKeys::new();
    for epoch in oldest..=current {
        let Ok(Some(key)) = fetch(store, epoch) else {
            continue;
        };
        if let Err(e) = keys.insert(epoch, key) {
            tracing::warn!(error = %e, epoch, "group_keys_insert_failed");
            return None;
        }
    }
    keys.get(current)?;
    Some(keys)
}

/// The key a batch sealed under `epoch` was signed with, public half, when `keys` holds that epoch.
pub(crate) fn verify_key(keys: &GroupKeys, epoch: u32) -> Option<DevicePublicKey> {
    keys.get(epoch)
        .map(|key| derive_group_op_signing_key(key).public_key())
}

#[cfg(test)]
#[path = "group_epoch_tests.rs"]
mod tests;
