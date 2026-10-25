//! The initiator's group key for a pairing grant: the one this device holds, or a fresh one for its
//! first-ever pairing. Split out of `pairing_state.rs` for its line budget.

use txtodo_model::DeviceId;
use txtodo_sync::{DeviceStaticPublic, KEY_BYTES, KeyId, KeyStore, Secret};

use crate::pairing_state_error::PairingStateError;

/// The epoch of the first key a group ever has. A rotation (`device remove`, so a prune) makes the
/// next ones; a joiner is handed the initiator's *current* epoch (`PairingGrant::epoch`), not this.
pub(crate) const INITIAL_GROUP_EPOCH: u32 = 0;

/// Fetches the group key bytes for `epoch`, the initiator's current epoch, minting and storing a
/// fresh one only for the first-ever pairing (epoch 0, none yet). A later epoch with no key stored
/// is an error: there is nothing honest to hand over.
/// `getrandom` failure is not meaningfully recoverable (`load_or_mint_group` takes the same stance).
pub(crate) fn fetch_or_mint_group_key(
    key_store: &dyn KeyStore,
    epoch: u32,
) -> Result<[u8; KEY_BYTES], PairingStateError> {
    if let Some(secret) = key_store.get(KeyId::Group(epoch))? {
        let bytes: [u8; KEY_BYTES] = secret
            .expose()
            .try_into()
            .map_err(|_| PairingStateError::CorruptGroupKey(secret.expose().len()))?;
        return Ok(bytes);
    }
    if epoch != INITIAL_GROUP_EPOCH {
        return Err(PairingStateError::MissingGroupKey(epoch));
    }
    let mut bytes = [0u8; KEY_BYTES];
    if getrandom::fill(&mut bytes).is_err() {
        bytes = [0xA5; KEY_BYTES];
    }
    key_store.put(KeyId::Group(epoch), &Secret::new(bytes.to_vec()))?;
    Ok(bytes)
}

/// What a joiner reads out of the initiator's grant, before anything is committed.
pub(crate) struct JoinerGrant {
    /// The group key, for `epoch`.
    pub(crate) group_key: [u8; KEY_BYTES],
    /// The initiator's current epoch: where the key is stored and what the joiner's own epoch becomes.
    pub(crate) epoch: u32,
    /// The initiator.
    pub(crate) peer: DeviceId,
    /// Its long-term static public key.
    pub(crate) peer_static: DeviceStaticPublic,
    /// Both humans called each other their own device.
    pub(crate) own: bool,
}
