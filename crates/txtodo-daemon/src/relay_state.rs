//! Shared state connecting `relay.rs`'s background relay transport task to `lan.rs`'s relay-
//! fallback dial path (plan M8 `sync-relay-enable`): the bound `RelayEndpoint`, once `relay.rs`
//! binds one. Same shape as `pairing_lan_state.rs`'s endpoint half, but for the relay carrier
//! instead of LAN — kept as its own type since relay binding is optional by design (no relay
//! configured is a normal, not-yet-bound state) where LAN's own endpoint failing to bind is only
//! ever a runtime fault.

use std::sync::{Arc, Mutex, PoisonError};

use txtodo_sync::RelayEndpoint;

/// Cheap to clone: one `Arc<Mutex<Option<Arc<RelayEndpoint>>>>`. Starts `None` — either relay was
/// never configured (`--relay` omitted) or `relay.rs` has not finished binding yet; `lan.rs`'s
/// relay-fallback dial treats both the same way (nothing to try).
#[derive(Clone, Default)]
pub(crate) struct RelayState {
    endpoint: Arc<Mutex<Option<Arc<RelayEndpoint>>>>,
}

impl RelayState {
    /// Records the endpoint `relay.rs` bound, so `lan.rs`'s fallback dial can reuse it rather than
    /// binding a second one.
    pub(crate) fn set(&self, endpoint: Arc<RelayEndpoint>) {
        *self.endpoint.lock().unwrap_or_else(PoisonError::into_inner) = Some(endpoint);
    }

    /// The bound relay endpoint, if relay is configured and `relay.rs` has bound one yet.
    pub(crate) fn get(&self) -> Option<Arc<RelayEndpoint>> {
        self.endpoint
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}
