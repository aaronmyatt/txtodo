//! The always-on, per-device-set control channel (task `daemon-workspace-identity-agreement`
//! stage 5): the first non-per-workspace background task in this codebase. Binds one
//! `RelayEndpoint` per *device*, using `DeviceIdentity`'s persisted relay identity (stage 1), so a
//! peer's durably-stored `relay_node_id` (stage 2) reaches this device regardless of which, if
//! any, workspace is open — the spawn point in `main.rs` is deliberately before
//! `WorkspaceCatalog::new`.
//!
//! Every control session (whether accepted or dialed) does the same symmetric thing: send this
//! device's currently-registered workspaces as `ControlMessage::Offer`s, then read whatever the
//! peer sends back until the connection goes idle (`IrohLink`'s own idle timeout closes it, the
//! same "short session, redial periodically" shape `lan.rs`/`relay.rs` already use) — an incoming
//! `Offer` lands in [`crate::workspace_offer_registry::WorkspaceOfferRegistry`]; `OfferAck`/
//! `Decline` are logged only (stage 6's own bookkeeping, not built here). A workspace already
//! adopted or actively registered under this device's own id is skipped, not re-offered forever.
//!
//! **Known, deliberately-not-solved gap this pass**: this endpoint shares its persisted identity
//! with every per-workspace relay endpoint (`relay.rs::bind`, stage 1) that happens to be live at
//! the same time — `relay.rs::build`'s ALPN set is identical on every relay endpoint this crate
//! binds, so two simultaneously-live endpoints under one identity is real, untested territory this
//! sandbox cannot exercise against a genuine relay server's routing behavior (no way to run one
//! here). Flagged for a human, same spirit as `relay_fallback.rs`'s own already-documented
//! identity-sharing gap; self-resolves once `daemon-shared-sync-link` consolidates every relay
//! surface onto one shared `Link`, which is explicitly out of scope for this task.

use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use tokio::sync::Semaphore;
use tokio::task::JoinHandle;
use txtodo_sync::RelayEndpoint;

use crate::device_identity::DeviceIdentity;
use crate::device_relay::DeviceRelay;
use crate::workspace_registry::WorkspaceRegistry;

/// How often the redial loop retries a known peer while unreachable, and — once reachable —
/// redials to pick up a workspace registered since the previous round. Same reasoning as
/// `relay.rs`'s `DIAL_KNOWN_PEER_INTERVAL`.
const DIAL_KNOWN_PEER_INTERVAL: Duration = Duration::from_millis(1_000);
/// Most control sessions (accepted or dialed) running at once — a hostile or buggy peer flooding
/// connections must not spawn unboundedly many blocking threads.
const MAX_CONCURRENT_CONTROL_SESSIONS: usize = 32;

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

/// Starts the control channel as a background task when `relay_url` names one — no relay
/// configured means no device-level control surface either, matching `relay.rs::start`'s own
/// "relay is opt-in" behaviour. `registry_path` is opened as its own, independent
/// `WorkspaceRegistry` handle (SQLite WAL mode already supports concurrent readers/writers safely,
/// the same property every restart-durability test in this crate already relies on) — this task
/// never shares the catalog's own handle, avoiding any restructuring of `WorkspaceCatalog` to
/// thread one through.
pub fn start(
    identity: Arc<DeviceIdentity>,
    relay_url: Option<String>,
    registry_path: PathBuf,
) -> Option<ControlChannelTransport> {
    let url = relay_url.filter(|u| !u.is_empty())?;
    Some(ControlChannelTransport {
        task: tokio::spawn(run(identity, url, registry_path)),
    })
}

async fn run(identity: Arc<DeviceIdentity>, url: String, registry_path: PathBuf) {
    let Some(registry) = open_registry(&registry_path) else {
        return;
    };
    let Some(device_relay) = DeviceRelay::bind(&identity, url).await else {
        return;
    };
    let endpoint = device_relay.endpoint();
    let sem = Arc::new(Semaphore::new(MAX_CONCURRENT_CONTROL_SESSIONS));
    tokio::join!(
        accept_loop(
            Arc::clone(&endpoint),
            Arc::clone(&identity),
            Arc::clone(&registry),
            Arc::clone(&sem)
        ),
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

async fn accept_loop(
    endpoint: Arc<RelayEndpoint>,
    identity: Arc<DeviceIdentity>,
    registry: Arc<Mutex<WorkspaceRegistry>>,
    sem: Arc<Semaphore>,
) {
    loop {
        match endpoint.accept().await {
            Ok(link) => handle_accepted(link, Arc::clone(&identity), Arc::clone(&registry), &sem),
            Err(e) => tracing::debug!(error = %e, "control_channel_accept_failed"),
        }
    }
}

fn handle_accepted(
    link: txtodo_sync::IrohLink,
    identity: Arc<DeviceIdentity>,
    registry: Arc<Mutex<WorkspaceRegistry>>,
    sem: &Arc<Semaphore>,
) {
    if link.alpn() != txtodo_sync::CONTROL_ALPN {
        return;
    }
    let Ok(permit) = Arc::clone(sem).try_acquire_owned() else {
        tracing::warn!(
            cap = MAX_CONCURRENT_CONTROL_SESSIONS,
            "control_channel_session_cap_reached_dropping_incoming"
        );
        return;
    };
    spawn_session(link, identity, registry, permit);
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

fn spawn_session(
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
