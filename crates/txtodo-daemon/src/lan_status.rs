//! Live LAN transport status (plan M4 `sync-lan-transport`), for `Health`/`txtodo doctor`. Owned
//! by the `Workspace` (not `lan.rs`) so `TxtodoService` needs no extra field to answer `Health`;
//! `lan.rs`'s background task updates it as it progresses. Split into its own file since it is
//! shared by both `workspace.rs` and `lan.rs`, neither of which should own the other's concern.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// Cheap to clone: two `Arc<AtomicBool>`s. Every workspace starts with both `false`; `lan.rs`
/// flips them once the corresponding step actually succeeds, never optimistically.
#[derive(Clone, Default)]
pub struct LanStatus {
    endpoint_bound: Arc<AtomicBool>,
    discovery_active: Arc<AtomicBool>,
}

impl LanStatus {
    /// The relay is disabled by construction (`bind_local_endpoint`'s own doc); this is a fact
    /// about the code, not a runtime state, but lives here so `Health` has one place to read every
    /// LAN fact from. M8 will need to make this a real flag when relay support lands.
    pub const RELAY_DISABLED: bool = true;

    /// Whether the local `iroh` endpoint bound successfully and is accepting connections.
    pub fn endpoint_bound(&self) -> bool {
        self.endpoint_bound.load(Ordering::Relaxed)
    }

    /// Whether mDNS is advertising and browsing for this workspace's sync group.
    pub fn discovery_active(&self) -> bool {
        self.discovery_active.load(Ordering::Relaxed)
    }

    pub(crate) fn set_endpoint_bound(&self, value: bool) {
        self.endpoint_bound.store(value, Ordering::Relaxed);
    }

    pub(crate) fn set_discovery_active(&self, value: bool) {
        self.discovery_active.store(value, Ordering::Relaxed);
    }
}
