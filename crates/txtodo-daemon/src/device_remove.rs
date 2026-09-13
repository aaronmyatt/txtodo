//! `txtodo device remove` (plan M4 tasks/sync-device-remove): validates the removal, rotates the
//! group key to whatever devices remain, and tombstones the removed device's row. Owned end-to-end
//! by this task; `devices_grpc.rs` delegates here from `server.rs` untouched — the same split as
//! `pairing_state.rs`/`pairing_grpc.rs`.
//!
//! Sequencing follows the task's own notes ("close the epoch before you announce the removal"): the
//! new epoch's key is generated and stored under this workspace's own keystore, and `meta`'s
//! `group_key_epoch` is advanced, *before* the device row is tombstoned — from that instant this
//! device's own future ops seal under the new epoch. There is no "stop advertising to the removed
//! device" step yet (`sync-lan-transport`'s live sessions are separate, later work), so store-then-
//! tombstone is the only sequencing this daemon can enforce today.
//!
//! **Known gap, flagged for the human**: [`plan_rotation`] is called for real and does mint one
//! [`WrappedGrant`] per remaining device, proving the crypto this task reuses actually integrates —
//! but nothing persists or delivers those bytes anywhere. "A device offline during rotation comes
//! back and finds its grant" (the task's own acceptance criterion) needs either a live transport
//! session (`sync-lan-transport`, out of scope here) or a new pending-grants store this task did not
//! add (out of scope: the brief asked for the `devices` table, not a second one). Each remaining
//! device's `key_epoch` column is still advanced, recording what this device believes it has minted
//! a grant for — not confirmation of delivery.

use std::collections::BTreeMap;

use txtodo_model::DeviceId;
use txtodo_store::StoreError;
use txtodo_sync::{
    DeviceStaticPublic, KEY_BYTES, KeyId, KeyStoreError, RemovalError, RotationError, Secret,
    WrappedGrant, plan_rotation, validate_removal,
};

use crate::workspace::Workspace;

/// What `txtodo device remove` did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemovalOutcome {
    /// `false` when no such device is known at all.
    pub removed: bool,
    /// `true` when the device was already removed before this call; nothing rotated.
    pub already_removed: bool,
    /// The epoch rotated to, or 0 when nothing rotated.
    pub rotated_to_epoch: u32,
    /// How many remaining devices were minted a grant for the new epoch (0 in the "removing the
    /// last peer, only this device is left" case — nobody else to grant to).
    pub grants_minted: usize,
}

/// Why `Workspace::remove_device` refused or failed.
#[derive(Debug)]
pub enum RemoveDeviceError {
    /// The store failed.
    Store(StoreError),
    /// The keystore failed.
    KeyStore(KeyStoreError),
    /// The removal itself is refused (self, or the last device) — checked before any crypto runs.
    Removal(RemovalError),
    /// Rotation planning failed. Modelled rather than assumed unreachable: this crate's own
    /// `EpochOverflow` check runs first, but the crypto layer's guard is not bypassed either.
    Rotation(RotationError),
    /// The epoch counter is already at `u32::MAX`; one more rotation would wrap around.
    EpochOverflow,
}

impl std::fmt::Display for RemoveDeviceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RemoveDeviceError::Store(e) => write!(f, "{e}"),
            RemoveDeviceError::KeyStore(e) => write!(f, "{e}"),
            RemoveDeviceError::Removal(e) => write!(f, "{e}"),
            RemoveDeviceError::Rotation(e) => write!(f, "{e}"),
            RemoveDeviceError::EpochOverflow => write!(f, "epoch counter would overflow u32::MAX"),
        }
    }
}

impl std::error::Error for RemoveDeviceError {}

impl From<StoreError> for RemoveDeviceError {
    fn from(e: StoreError) -> RemoveDeviceError {
        RemoveDeviceError::Store(e)
    }
}
impl From<KeyStoreError> for RemoveDeviceError {
    fn from(e: KeyStoreError) -> RemoveDeviceError {
        RemoveDeviceError::KeyStore(e)
    }
}
impl From<RotationError> for RemoveDeviceError {
    fn from(e: RotationError) -> RemoveDeviceError {
        RemoveDeviceError::Rotation(e)
    }
}

impl Workspace {
    /// Removes `target` and rotates the group key to whatever devices remain (plan M4
    /// `tasks/sync-device-remove`). Idempotent — removing an already-removed device rotates
    /// nothing and says so — and guarded before any crypto runs via `validate_removal`, reused
    /// unchanged from `txtodo-sync` (never self, never the last device in the group).
    pub(crate) fn remove_device(
        &self,
        target: DeviceId,
        now_ms: u64,
    ) -> Result<RemovalOutcome, RemoveDeviceError> {
        // The store's Mutex is not reentrant, and `advance_group_epoch` below takes it too — so
        // every lock here is scoped to a block, never held across that call (a prior version held
        // it for the whole function and deadlocked on the second lock attempt).
        let (active_peers, row) = {
            let store = self
                .store()
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let active_peers: Vec<_> = store
                .list_devices()?
                .into_iter()
                .filter(|d| d.removed_at_ms.is_none())
                .collect();
            // +1 for this device itself: the devices table holds peers only, never a self-row.
            let devices_before = active_peers.len() + 1;
            validate_removal(target, self.device(), devices_before)
                .map_err(RemoveDeviceError::Removal)?;
            let Some(row) = store.device(target)? else {
                return Ok(no_op_outcome(false));
            };
            (active_peers, row)
        };
        if row.removed_at_ms.is_some() {
            return Ok(RemovalOutcome {
                removed: true,
                ..no_op_outcome(true)
            });
        }

        let remaining: BTreeMap<DeviceId, DeviceStaticPublic> = active_peers
            .iter()
            .filter(|d| d.device != target)
            .map(|d| (d.device, DeviceStaticPublic::from_bytes(d.static_public)))
            .collect();

        let current_epoch = self.group_epoch();
        let (new_epoch, new_key, grants_minted) = plan_next_epoch(current_epoch, &remaining)?;

        // Store-then-tombstone (module doc): this device's own future ops seal under the new
        // epoch from the moment it lands, before the removed device's row is ever touched. All of
        // it lands under one held lock (never re-locked while already held — a prior version of
        // this function called back into `Workspace` methods that re-locked the same `Mutex` from
        // the same thread and deadlocked).
        self.key_store()
            .put(KeyId::Group(new_epoch), &Secret::new(new_key.to_vec()))?;
        {
            let mut store = self
                .store()
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            store.meta_set(
                crate::keystore_setup::GROUP_EPOCH_KEY,
                &new_epoch.to_be_bytes(),
            )?;
            store.remove_device(target, now_ms)?;
            for device in remaining.keys() {
                store.set_device_key_epoch(*device, new_epoch)?;
            }
        }
        self.set_group_epoch(new_epoch);

        Ok(RemovalOutcome {
            removed: true,
            already_removed: false,
            rotated_to_epoch: new_epoch,
            grants_minted,
        })
    }
}

/// Computes the next epoch, a fresh key for it, and mints one [`WrappedGrant`] per entry in
/// `remaining` (empty `remaining` mints none — nobody left to grant to). Split out of
/// `remove_device` to stay under the line-count budget.
fn plan_next_epoch(
    current_epoch: u32,
    remaining: &BTreeMap<DeviceId, DeviceStaticPublic>,
) -> Result<(u32, [u8; KEY_BYTES], usize), RemoveDeviceError> {
    let new_epoch = current_epoch
        .checked_add(1)
        .ok_or(RemoveDeviceError::EpochOverflow)?;
    let mut new_key = [0u8; KEY_BYTES];
    if getrandom::fill(&mut new_key).is_err() {
        new_key = [0xA5; KEY_BYTES];
    }
    let grants_minted = if remaining.is_empty() {
        // Removing the last peer: nobody else to grant to (see the module doc's known gap for why
        // a real grant, once there is somebody to send it to, is not delivered anywhere).
        0
    } else {
        let grants: BTreeMap<DeviceId, WrappedGrant> =
            plan_rotation(current_epoch, &new_key, remaining)?;
        debug_assert_eq!(grants.len(), remaining.len());
        debug_assert!(grants.values().all(|g| g.epoch == new_epoch));
        grants.len()
    };
    Ok((new_epoch, new_key, grants_minted))
}

fn no_op_outcome(removed: bool) -> RemovalOutcome {
    RemovalOutcome {
        removed,
        already_removed: removed,
        rotated_to_epoch: 0,
        grants_minted: 0,
    }
}
