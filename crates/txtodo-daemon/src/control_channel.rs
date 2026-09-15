//! The always-on, per-device-set control channel (task `daemon-workspace-identity-agreement`
//! stage 5): the first non-per-workspace background task in this codebase. Runs over this
//! device's one shared `RelayEndpoint`, bound once in `main.rs::run` (task `daemon-shared-sync-
//! link` stage 5, [`DeviceRelay::bind`]) *before* this module or `WorkspaceCatalog::new` ever
//! runs, so a peer's durably-stored `relay_node_id` (stage 2 of the prerequisite task) reaches
//! this device regardless of which, if any, workspace is open. This module no longer binds
//! anything itself — it receives the already-bound [`DeviceRelay`] as a plain argument, the same
//! one every open workspace's own `relay::start` shares.
//!
//! Every control session (whether accepted or dialed) does the same symmetric thing: send this
//! device's currently-registered workspaces as `ControlMessage::Offer`s, then read whatever the
//! peer sends back until the connection goes idle (`IrohLink`'s own idle timeout closes it, the
//! same "short session, redial periodically" shape `lan.rs`/`relay.rs` already use) — an incoming
//! `Offer` lands in [`crate::workspace_offer_registry::WorkspaceOfferRegistry`]; `OfferAck`/
//! `Decline` are logged only (stage 6's own bookkeeping, not built here). A workspace already
//! adopted or actively registered under this device's own id is skipped, not re-offered forever.
//!
//! The accept loop no longer silently drops a connection that did not negotiate `CONTROL_ALPN` —
//! [`control_dispatch`] dispatches all three ALPNs this device's one endpoint accepts, routing a
//! sync connection to the right open workspace by peeking its first frame's clear `workspace_id`
//! (`txtodo_sync::peek_workspace`). See `control_dispatch.rs`'s own module doc for the full
//! routing story — this is what actually resolves the identity-sharing collision this module used
//! to flag as its own known, deliberately-not-solved gap.

use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use tokio::sync::Semaphore;
use tokio::task::JoinHandle;
use txtodo_sync::RelayEndpoint;

use crate::control_dispatch::{self, DispatchCtx};
use crate::device_identity::DeviceIdentity;
use crate::device_relay::DeviceRelay;
use crate::workspace_registry::WorkspaceRegistry;

/// How often the redial loop retries a known peer while unreachable, and — once reachable —
/// redials to pick up a workspace registered since the previous round. Same reasoning as
/// `relay.rs`'s `DIAL_KNOWN_PEER_INTERVAL`.
const DIAL_KNOWN_PEER_INTERVAL: Duration = Duration::from_millis(1_000);
/// Most sessions of any kind (control, pairing or sync) this device's one shared endpoint accepts
/// or dials at once — a hostile or buggy peer flooding connections must not spawn unboundedly many
/// blocking threads. Named for what it now bounds (task `daemon-shared-sync-link` stage 3: this
/// endpoint stopped being control-only the moment `control_dispatch.rs` started routing the other
/// two ALPNs through it too).
const MAX_CONCURRENT_ACCEPTED_SESSIONS: usize = 32;

/// The background control-channel task; `abort()` on daemon shutdown, same pattern as
/// `RelayTransport`/`LanTransport`.
pub struct ControlChannelTransport {
    task: JoinHandle<()>,
}

impl ControlChannelTransport {
    /// Stops the control channel. Best-effort: the task may already have exited (bind failed).
    pub fn abort(&self) {
        self.task.abort();
    }
}

/// Starts the control channel as a background task when `device_relay` names an already-bound
/// one — no relay configured (or a failed bind) means no device-level control surface either,
/// matching `relay.rs::start`'s own "relay is opt-in" behaviour. `registry_path` is opened as its
/// own, independent `WorkspaceRegistry` handle (SQLite WAL mode already supports concurrent
/// readers/writers safely, the same property every restart-durability test in this crate already
/// relies on) — this task never shares the catalog's own handle, avoiding any restructuring of
/// `WorkspaceCatalog` to thread one through.
pub fn start(
    identity: Arc<DeviceIdentity>,
    device_relay: Option<Arc<DeviceRelay>>,
    registry_path: PathBuf,
) -> Option<ControlChannelTransport> {
    let device_relay = device_relay?;
    Some(ControlChannelTransport {
        task: tokio::spawn(run(identity, device_relay, registry_path)),
    })
}

async fn run(
    identity: Arc<DeviceIdentity>,
    device_relay: Arc<DeviceRelay>,
    registry_path: PathBuf,
) {
    let Some(registry) = open_registry(&registry_path) else {
        return;
    };
    let ctx = DispatchCtx {
        identity: Arc::clone(&identity),
        registry: Arc::clone(&registry),
        device_relay: Arc::clone(&device_relay),
    };
    let endpoint = device_relay.endpoint();
    let sem = Arc::new(Semaphore::new(MAX_CONCURRENT_ACCEPTED_SESSIONS));
    tokio::join!(
        accept_loop(Arc::clone(&endpoint), ctx, Arc::clone(&sem)),
        redial_loop(endpoint, identity, registry, sem),
    );
}

fn open_registry(registry_path: &std::path::Path) -> Option<Arc<Mutex<WorkspaceRegistry>>> {
    match WorkspaceRegistry::open(registry_path) {
        Ok(r) => Some(Arc::new(Mutex::new(r))),
        Err(e) => {
            tracing::warn!(error = %e, "control_channel_registry_open_failed");
            None
        }
    }
}

async fn accept_loop(endpoint: Arc<RelayEndpoint>, ctx: DispatchCtx, sem: Arc<Semaphore>) {
    loop {
        match endpoint.accept().await {
            Ok(link) => {
                control_dispatch::accept_one(link, &ctx, &sem, MAX_CONCURRENT_ACCEPTED_SESSIONS);
            }
            Err(e) => tracing::debug!(error = %e, "control_channel_accept_failed"),
        }
    }
}

/// Dials every known peer with a durably-stored relay node id (`IdentityStore::list_devices()`,
/// stage 2), redialing every [`DIAL_KNOWN_PEER_INTERVAL`] whether or not the previous round
/// connected — same "short session, periodic redial" shape as `relay.rs::dial_known_peer`. A
/// device paired before stage 2 landed has no `relay_node_id` and simply never appears here
/// (documented gap, no migration, same precedent `device_identity.rs` already set).
async fn redial_loop(
    endpoint: Arc<RelayEndpoint>,
    identity: Arc<DeviceIdentity>,
    registry: Arc<Mutex<WorkspaceRegistry>>,
    sem: Arc<Semaphore>,
) {
    let mut ticker = tokio::time::interval(DIAL_KNOWN_PEER_INTERVAL);
    loop {
        ticker.tick().await;
        for node_id in known_relay_peers(&identity) {
            let Ok(permit) = Arc::clone(&sem).try_acquire_owned() else {
                break;
            };
            dial_one_peer(
                &endpoint,
                node_id,
                Arc::clone(&identity),
                Arc::clone(&registry),
                permit,
            )
            .await;
        }
    }
}

async fn dial_one_peer(
    endpoint: &RelayEndpoint,
    node_id: [u8; 32],
    identity: Arc<DeviceIdentity>,
    registry: Arc<Mutex<WorkspaceRegistry>>,
    permit: tokio::sync::OwnedSemaphorePermit,
) {
    match endpoint.connect_control(node_id).await {
        Ok(link) => spawn_session(link, identity, registry, permit),
        Err(e) => tracing::debug!(error = %e, "control_channel_dial_known_peer_failed"),
    }
}

fn known_relay_peers(identity: &DeviceIdentity) -> Vec<[u8; 32]> {
    let store = identity
        .store()
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    store
        .list_devices()
        .unwrap_or_default()
        .into_iter()
        .filter(|d| d.removed_at_ms.is_none())
        .filter_map(|d| d.relay_node_id)
        .collect()
}

/// `pub(crate)`: `control_dispatch.rs`'s `CONTROL_ALPN` branch calls this directly, the same
/// session-driving code an accepted or a dialed control connection always ran.
pub(crate) fn spawn_session(
    link: txtodo_sync::IrohLink,
    identity: Arc<DeviceIdentity>,
    registry: Arc<Mutex<WorkspaceRegistry>>,
    permit: tokio::sync::OwnedSemaphorePermit,
) {
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        let mut link = link;
        crate::control_session::drive_control_session(&mut link, &identity, &registry);
    });
}
