//! `Workspace::register_paired_device`, the initiator-side twin of `Workspace::adopt_group_key`'s
//! joiner-side device registration. Its own file for the line budgets of both `workspace.rs` and
//! `pairing_lan.rs` (the same split `device_remove.rs` makes for its own `impl Workspace`).

use txtodo_model::DeviceId;

use crate::workspace::Workspace;

impl Workspace {
    /// Registers a peer's long-term static public key in the device-global `devices` table (ADR
    /// 0021 — the same table `adopt_group_key`'s joiner-side twin, `device_remove.rs` and
    /// `devices_grpc.rs` all use via `identity_store()`). Previously locked `store()` instead — the
    /// per-workspace oplog store, a different SQLite database nothing else reads a device row
    /// from — so every registration through this path landed somewhere `txtodo device list`/
    /// `devices_grpc.rs` never looked, silently. Caught by `pairing_lan_tests.rs`'s
    /// `process_hello_registers_the_joiner_in_the_initiators_devices_table`.
    /// `own`: both humans called each other their own device (task
    /// default-workspace-pairing-consent); the default workspace merges only with such a peer.
    pub(crate) fn register_paired_device(
        &self,
        device: DeviceId,
        static_public: [u8; txtodo_sync::DEVICE_STATIC_KEY_BYTES],
        now_ms: u64,
        own: bool,
    ) -> Result<(), txtodo_store::StoreError> {
        let mut store = self
            .identity_store()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let new = txtodo_store::NewDevice {
            device,
            name: String::new(),
            static_public,
            paired_at_ms: now_ms,
            last_known_wall_ms: None,
            key_epoch: 0,
        };
        store.register_device_as(&new, own)?;
        // A peer parked for holding another group's key shares ours now (task sync-drift line 5).
        self.peer_keys().forget(device, "paired");
        Ok(())
    }
}
