//! `Workspace::register_paired_device`, the initiator-side twin of `Workspace::adopt_group_key`'s
//! joiner-side device registration. Its own file for the line budgets of both `workspace.rs` and
//! `pairing_lan.rs` (the same split `device_remove.rs` makes for its own `impl Workspace`).

use txtodo_model::DeviceId;

use crate::workspace::Workspace;

impl Workspace {
    /// Registers a peer's long-term static public key in this workspace's own `devices` table.
    pub(crate) fn register_paired_device(
        &self,
        device: DeviceId,
        static_public: [u8; txtodo_sync::DEVICE_STATIC_KEY_BYTES],
        now_ms: u64,
    ) -> Result<(), txtodo_store::StoreError> {
        let mut store = self
            .store()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        store.register_device(&txtodo_store::NewDevice {
            device,
            name: String::new(),
            static_public,
            paired_at_ms: now_ms,
            last_known_wall_ms: None,
            key_epoch: 0,
        })
    }
}
