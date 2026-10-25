//! What a device remembers about each group-key epoch it has held (task `peer-prune`, ADR 0044):
//! who minted the key, and which devices the rotation dropped. Kept in the identity store's `meta`
//! as `group_epoch_info:<epoch>`, so a grant can be made again for a peer that comes back later
//! (the key is in the keystore, the rest is here), and two devices that each rotate to the same
//! epoch can settle it: the lower minter id wins.
//!
//! Layout: the minter's 16 bytes, then 16 bytes per dropped device, all big-endian ULIDs. Epoch 0,
//! and a key adopted through pairing, have no record: nobody dropped anything to make them.

use txtodo_model::{DeviceId, Ulid};
use txtodo_store::{IdentityStore, StoreError};

/// Most dropped devices one record keeps; the control message that carries them has the same cap.
const MAX_LEFT_OUT: usize = txtodo_sync::MAX_OWN_DEVICES;

/// Who minted an epoch's key and whom the rotation dropped.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct EpochInfo {
    /// The device that made the key.
    pub(crate) minter: DeviceId,
    /// The devices the rotation that made it dropped.
    pub(crate) left_out: Vec<DeviceId>,
}

fn key(epoch: u32) -> String {
    format!("group_epoch_info:{epoch}")
}

fn id_bytes(device: DeviceId) -> [u8; 16] {
    device.ulid().to_u128().to_be_bytes()
}

fn id_of(chunk: &[u8]) -> Option<DeviceId> {
    let raw: [u8; 16] = chunk.try_into().ok()?;
    Some(DeviceId::new(Ulid::from_u128(u128::from_be_bytes(raw))))
}

impl EpochInfo {
    fn encode(&self) -> Vec<u8> {
        let mut out = id_bytes(self.minter).to_vec();
        for device in self.left_out.iter().take(MAX_LEFT_OUT) {
            out.extend_from_slice(&id_bytes(*device));
        }
        out
    }

    fn decode(bytes: &[u8]) -> Option<EpochInfo> {
        if bytes.len() < 16 || !bytes.len().is_multiple_of(16) {
            return None;
        }
        let mut chunks = bytes.chunks_exact(16);
        let minter = id_of(chunks.next()?)?;
        let left_out = chunks.filter_map(id_of).collect();
        Some(EpochInfo { minter, left_out })
    }
}

/// The record for `epoch`, if this device made or adopted one.
pub(crate) fn read(store: &IdentityStore, epoch: u32) -> Option<EpochInfo> {
    let bytes = store.meta_get(&key(epoch)).ok().flatten()?;
    EpochInfo::decode(&bytes)
}

/// Records `info` for `epoch`, replacing any earlier record (a lost tie-break is replaced by the
/// winner's).
pub(crate) fn write(
    store: &mut IdentityStore,
    epoch: u32,
    info: &EpochInfo,
) -> Result<(), StoreError> {
    store.meta_set(&key(epoch), &info.encode())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn device(n: u128) -> DeviceId {
        DeviceId::new(Ulid::from_u128(n))
    }

    #[test]
    fn a_record_round_trips_and_a_malformed_one_reads_as_none() {
        let info = EpochInfo {
            minter: device(7),
            left_out: vec![device(1), device(2)],
        };
        assert_eq!(EpochInfo::decode(&info.encode()), Some(info));
        let alone = EpochInfo {
            minter: device(7),
            left_out: vec![],
        };
        assert_eq!(EpochInfo::decode(&alone.encode()), Some(alone));
        assert_eq!(EpochInfo::decode(&[]), None);
        assert_eq!(EpochInfo::decode(&[0; 17]), None);
    }

    #[test]
    fn records_live_in_meta_per_epoch() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = IdentityStore::open(&dir.path().join("identity.db")).unwrap();
        assert_eq!(read(&store, 3), None);
        let info = EpochInfo {
            minter: device(9),
            left_out: vec![device(4)],
        };
        write(&mut store, 3, &info).unwrap();
        assert_eq!(read(&store, 3), Some(info));
        assert_eq!(read(&store, 2), None, "another epoch has none");
    }
}
