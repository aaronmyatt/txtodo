//! `Workspace::adopt_group_key`, the joiner-side twin of `pairing_register.rs`'s initiator-side
//! device registration. Its own file for the line budgets of both `workspace.rs` and
//! `pairing_register.rs` itself (the same split `device_remove.rs` makes for its own `impl
//! Workspace`).

use txtodo_store::NewDevice;
use txtodo_sync::GroupId;

use crate::pairing_state_error::PairingStateError;
use crate::workspace::Workspace;

impl Workspace {
    /// Finishes a pairing on the joiner's side: unwraps the sealed [`txtodo_sync::PairingGrant`]
    /// the initiator sent, stores the group key under this device's keystore, registers the
    /// initiator's static public key in the device-global `devices` table (plan M4
    /// `sync-device-remove`; see `pairing_state.rs::preview_group_key`'s doc for the leg it does
    /// not), and adopts `group` as this device's own — atomically, so it never claims a group
    /// without also holding its key. Called by `pairing_lan.rs::finish_joiner` in production and
    /// by `pairing_grpc_tests.rs` directly (whitebox).
    pub(crate) fn adopt_group_key(
        &self,
        group: GroupId,
        sealed: &[u8],
        now_ms: u64,
    ) -> Result<(), PairingStateError> {
        // Unwrap first (a pure read — no key-store write, no clearing of the pairing attempt), so
        // a failure below leaves the attempt retryable from scratch instead of stranding an
        // already-committed key with no matching device/group row. See
        // pairing_state.rs::preview_group_key.
        let (group_key, peer_device, peer_static) =
            self.pairing().preview_group_key(sealed, now_ms)?;
        let mut store = self
            .identity_store()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        store.meta_set(crate::device_identity::GROUP_ID_KEY, &group.0.to_be_bytes())?;
        store.register_device(&NewDevice {
            device: peer_device,
            name: String::new(),
            static_public: peer_static.to_bytes(),
            paired_at_ms: now_ms,
            last_known_wall_ms: None,
            key_epoch: 0,
        })?;
        drop(store);
        // Point of no return: only now do we commit the key and end the pairing attempt.
        self.pairing()
            .commit_group_key(self.key_store().as_ref(), group_key, now_ms)?;
        self.set_group(group);
        Ok(())
    }
}
