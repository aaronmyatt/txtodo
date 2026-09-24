//! The initiator's group key for a pairing grant: the one this device holds, or a fresh one for its
//! first-ever pairing. Split out of `pairing_state.rs` for its line budget.

use txtodo_sync::{KEY_BYTES, KeyId, KeyStore, Secret};

use crate::pairing_state_error::PairingStateError;

/// The group-key epoch pairing establishes. Rotation (`txtodo device remove`) is future work.
pub(crate) const INITIAL_GROUP_EPOCH: u32 = 0;

/// Fetches the device group key bytes, minting and storing a fresh one for the first-ever pairing.
/// `getrandom` failure is not meaningfully recoverable (`load_or_mint_group` takes the same stance).
pub(crate) fn fetch_or_mint_group_key(
    key_store: &dyn KeyStore,
) -> Result<[u8; KEY_BYTES], PairingStateError> {
    if let Some(secret) = key_store.get(KeyId::Group(INITIAL_GROUP_EPOCH))? {
        let bytes: [u8; KEY_BYTES] = secret
            .expose()
            .try_into()
            .map_err(|_| PairingStateError::CorruptGroupKey(secret.expose().len()))?;
        return Ok(bytes);
    }
    let mut bytes = [0u8; KEY_BYTES];
    if getrandom::fill(&mut bytes).is_err() {
        bytes = [0xA5; KEY_BYTES];
    }
    key_store.put(
        KeyId::Group(INITIAL_GROUP_EPOCH),
        &Secret::new(bytes.to_vec()),
    )?;
    Ok(bytes)
}
