//! One relay endpoint per device (task `daemon-shared-sync-link` stage 2), shared by the
//! always-on control channel and every open workspace's own relay sync instead of each binding
//! its own — `control_channel.rs`'s own module doc names the bug this replaces: two (or more)
//! `iroh::Endpoint::bind()` calls under the same persisted identity
//! (`DeviceIdentity::relay_identity()`) make a real relay server refuse the second connection
//! outright ("Another endpoint connected with the same endpoint id"), reproduced for real against
//! n0's public relay by `relay_converge.rs`/`pairing_relay.rs`. `DeviceRelay::bind` absorbs what
//! was `control_channel.rs`'s own private `bind_endpoint`, unchanged in behaviour.
//!
//! [`WorkspaceRoutes`] (`register`/`unregister`/`route`) is what `control_dispatch.rs`'s shared
//! accept loop (stage 3) consults to tell an incoming sync-`ALPN` connection's peeked
//! `workspace_id` (`txtodo_sync::peek_workspace`) apart from every other open workspace's, and to
//! find *some* open workspace to hand a `PAIRING_ALPN` connection to (pairing only ever touches
//! the shared `DeviceIdentity` underneath, ADR 0021, so any open workspace's handle will do). Kept
//! as its own type, bundled into `DeviceRelay` rather than merged into its fields directly, so the
//! table's own logic stays testable without a real network bind (`device_relay_tests.rs`);
//! `DeviceRelay::bind`'s own networking is exercised indirectly by every real-daemon relay test in
//! `tests/` instead. `register`/`unregister` gain their first production caller in stage 5, when
//! `workspace_catalog_open.rs` starts keeping this table in sync with which workspaces are open.

use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard};

use txtodo_model::DeviceId;
use txtodo_store::WorkspaceId;
use txtodo_sync::{GroupId, MAX_RELAY_PEERS, RelayConfig, RelayEndpoint};

use crate::device_identity::DeviceIdentity;
use crate::pairing_wire::hex_encode;
use crate::server::SharedWorkspace;

/// Most workspaces this device routes incoming sync connections to at once — a device realistically
/// opens far fewer than this; the cap exists so nothing here can grow without limit (every
/// collection in this crate has a named, checked cap).
pub const MAX_ROUTED_WORKSPACES: usize = 256;

/// What an accept loop needs to hand an incoming, already-routed connection to the existing,
/// unchanged `lan::spawn_driver(ws, device, group, link, permit)` — the exact inputs it already
/// takes, bundled so `register` stays under the argument-count cap.
#[derive(Clone)]
pub struct WorkspaceRoute {
    /// The live workspace this route resolves to.
    pub ws: SharedWorkspace,
    /// This device's own id — same for every route (ADR 0021), carried per-route so a caller never
    /// needs a second lookup to reach it.
    pub device: DeviceId,
    /// The shared sync group — same for every route (ADR 0021), same reasoning as `device`.
    pub group: GroupId,
}

/// Why registering a route failed.
#[derive(Debug)]
pub enum DeviceRelayError {
    /// A genuinely new workspace would exceed [`MAX_ROUTED_WORKSPACES`]; re-registering an
    /// already-routed id (e.g. a redundant open) never hits this.
    TooManyRoutes,
}

impl fmt::Display for DeviceRelayError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DeviceRelayError::TooManyRoutes => {
                write!(f, "already routing {MAX_ROUTED_WORKSPACES} workspaces")
            }
        }
    }
}

impl std::error::Error for DeviceRelayError {}

/// Which open workspace an incoming, already-peeked `workspace_id` resolves to — pure data, no
/// networking, deliberately independent of whether a relay endpoint is even bound.
#[derive(Default)]
pub struct WorkspaceRoutes {
    inner: RwLock<HashMap<WorkspaceId, WorkspaceRoute>>,
}

impl WorkspaceRoutes {
    /// An empty table.
    pub fn new() -> WorkspaceRoutes {
        WorkspaceRoutes::default()
    }

    /// Registers `route` under `id`. Idempotent-in-place for an id already routed (a workspace
    /// reopened, or a redundant registration) — only a genuinely new id counts against
    /// [`MAX_ROUTED_WORKSPACES`].
    pub fn register(&self, id: WorkspaceId, route: WorkspaceRoute) -> Result<(), DeviceRelayError> {
        let mut routes = self.write();
        if !routes.contains_key(&id) && routes.len() >= MAX_ROUTED_WORKSPACES {
            return Err(DeviceRelayError::TooManyRoutes);
        }
        routes.insert(id, route);
        Ok(())
    }

    /// Removes `id`'s route, if any — called when a workspace closes (`OpenedWorkspace::Drop`,
    /// stage 5), so a connection for a no-longer-open workspace is never routed to a stale handle.
    pub fn unregister(&self, id: WorkspaceId) {
        self.write().remove(&id);
    }

    /// The route for `id`, if this device currently has that workspace open. `None` for an
    /// unknown, not-yet-open, or already-closed workspace — the caller (`control_dispatch.rs`'s
    /// accept loop) logs and drops the connection rather than treating this as fatal.
    pub fn route(&self, id: WorkspaceId) -> Option<WorkspaceRoute> {
        self.read().get(&id).cloned()
    }

    /// Any one currently-open workspace's route, if at least one is open — `PAIRING_ALPN`'s own
    /// handler (`pairing_lan::handle_incoming_over`) only ever reaches the shared `DeviceIdentity`
    /// through its `&SharedWorkspace` argument (ADR 0021), so which specific one it gets does not
    /// matter. `None` when no workspace is open at all; the caller logs and drops the connection.
    pub fn any(&self) -> Option<WorkspaceRoute> {
        self.read().values().next().cloned()
    }

    fn read(&self) -> RwLockReadGuard<'_, HashMap<WorkspaceId, WorkspaceRoute>> {
        self.inner
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn write(&self) -> RwLockWriteGuard<'_, HashMap<WorkspaceId, WorkspaceRoute>> {
        self.inner
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

/// This device's one relay endpoint plus the routing table its accept loop consults. One per
/// `txtodod` process, bound once, before any workspace opens — mirrors `DeviceIdentity`'s own
/// device-level (not per-workspace) lifetime.
pub struct DeviceRelay {
    endpoint: Arc<RelayEndpoint>,
    routes: WorkspaceRoutes,
}

impl DeviceRelay {
    /// Binds one `RelayEndpoint` at `url` using `identity`'s persisted relay identity — the same
    /// call `control_channel.rs`'s own private `bind_endpoint` used to make, moved here so it is
    /// the single place any relay-based transport on this device ever binds from. `None` (logged)
    /// on a bind failure, same "logged, never fatal" precedent as every other transport bind in
    /// this crate.
    pub async fn bind(identity: &DeviceIdentity, url: String) -> Option<Arc<DeviceRelay>> {
        let cfg = RelayConfig {
            url,
            max_peers: MAX_RELAY_PEERS,
        };
        let bound =
            RelayEndpoint::bind_with_secret_key(&cfg, identity.group(), identity.relay_identity())
                .await;
        on_bind_result(bound)
    }

    /// The bound endpoint, shared by every caller that needs to connect or accept over it (the
    /// control channel's accept/redial loops today; a workspace's own dial loop and the shared
    /// accept loop from stage 3 onward).
    pub fn endpoint(&self) -> Arc<RelayEndpoint> {
        Arc::clone(&self.endpoint)
    }

    /// The routing table an accept loop consults to dispatch an incoming sync or pairing
    /// connection to the right open workspace.
    pub fn routes(&self) -> &WorkspaceRoutes {
        &self.routes
    }
}

fn on_bind_result(
    bound: Result<RelayEndpoint, txtodo_sync::RelayError>,
) -> Option<Arc<DeviceRelay>> {
    let endpoint = match bound {
        Ok(e) => e,
        Err(e) => {
            log_bind_failed(&e);
            return None;
        }
    };
    log_bound(&hex_encode(&endpoint.node_id_bytes()));
    Some(Arc::new(DeviceRelay {
        endpoint: Arc::new(endpoint),
        routes: WorkspaceRoutes::new(),
    }))
}

fn log_bind_failed(e: &txtodo_sync::RelayError) {
    tracing::warn!(error = %e, "device_relay_bind_failed");
}

fn log_bound(node_id: &str) {
    tracing::info!(node_id, "device_relay_bound");
}
