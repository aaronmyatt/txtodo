//! Wires `txtodo-sync`'s `Discovery` and `LanEndpoint` into a running `txtodod` (plan M4
//! `sync-lan-transport`): finds other daemons for the same sync group on the LAN and drives a
//! `Session` over the real transport with each one. Never fatal to the daemon — a bind or
//! discovery failure is logged and this device simply runs without LAN sync, the same as a fresh
//! workspace with no group key yet. `iroh`/`mdns-sd` never appear here or anywhere else in this
//! crate; only `txtodo_sync`'s own types do (`.claude/budgets.json`'s `allowedDeps`).
//!
//! **Known limitation, not a bug in this module**: two endpoints on the very same host cannot
//! actually complete a QUIC connection right now (`txtodo-sync`'s `endpoint_tests.rs` and
//! `CLAUDE.md` have the full diagnosis) — a real LAN with two distinct machines is not expected to
//! hit it. This module dials and accepts in good faith regardless; on this sandbox a dial simply
//! times out or fails, logged and left to the next discovery re-announcement, exactly like any
//! other transient LAN failure would be handled.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::Semaphore;
use tokio::task::JoinHandle;
use txtodo_model::DeviceId;
use txtodo_sync::{
    DiscoveredPeer, Discovery, GroupId, IrohLink, LanEndpoint, PeerEvent, PeerTable, Sighting,
    backoff_ms,
};

use crate::clock::Clock;
use crate::lan_session::{drive_session, read};
use crate::server::SharedWorkspace;

/// Refuses a 101st concurrent sync session the same way `MAX_LAN_PEERS` bounds the peer table
/// itself — a LAN flooded with peers must not spawn unbounded tasks.
pub const MAX_CONCURRENT_LAN_SESSIONS: usize = 16;

/// Bounded: a real connect that never resolves (a black-holed peer) cannot hang this forever.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// The background LAN transport task; `abort()` on daemon shutdown, same pattern as
/// `watch_task`'s `JoinHandle`.
pub struct LanTransport {
    task: JoinHandle<()>,
}

impl LanTransport {
    /// Stops the LAN transport. Best-effort: the task may already have exited (bind or discovery
    /// failed).
    pub fn abort(&self) {
        self.task.abort();
    }
}

/// Starts LAN discovery and the iroh endpoint as a background task.
pub fn start(ws: SharedWorkspace, clock: Arc<dyn Clock>) -> LanTransport {
    LanTransport {
        task: tokio::spawn(run(ws, clock)),
    }
}

/// Attempt/backoff bookkeeping per peer: when we last dialed, and how many failures in a row.
#[derive(Default)]
struct DialState {
    last_attempt_ms: BTreeMap<DeviceId, u64>,
    failures: BTreeMap<DeviceId, u32>,
}

impl DialState {
    /// Whether enough time has passed since the last attempt at `peer`, per `backoff_ms`.
    fn due(&self, peer: DeviceId, now_ms: u64) -> bool {
        match self.last_attempt_ms.get(&peer) {
            None => true,
            Some(&last) => {
                let attempt = self.failures.get(&peer).copied().unwrap_or(0);
                now_ms.saturating_sub(last) >= backoff_ms(attempt)
            }
        }
    }

    fn record_attempt(&mut self, peer: DeviceId, now_ms: u64) {
        self.last_attempt_ms.insert(peer, now_ms);
    }

    fn record_failure(&mut self, peer: DeviceId) {
        *self.failures.entry(peer).or_insert(0) += 1;
    }

    fn record_success(&mut self, peer: DeviceId) {
        self.failures.remove(&peer);
    }
}

/// This device's identity for the sync group, plus the workspace — bundled so no function below
/// needs more than `maxParams` arguments, and cheap to clone (an `Arc` and two `Copy` ids).
#[derive(Clone)]
struct LanCtx {
    ws: SharedWorkspace,
    device: DeviceId,
    group: GroupId,
}

/// Everything a bind/discover/browse setup produces, kept alive for the run loop's whole life.
struct LanSetup {
    endpoint: Arc<LanEndpoint>,
    discovery: Discovery,
    browse: txtodo_sync::BrowseEvents,
    ctx: LanCtx,
}

async fn setup(ws: SharedWorkspace) -> Option<LanSetup> {
    let endpoint = Arc::new(bind_endpoint().await?);
    let (device, group, status) = {
        let ws = read(&ws);
        (ws.device(), ws.group(), ws.lan_status().clone())
    };
    status.set_endpoint_bound(true);
    let discovery = start_discovery(device, group, &endpoint)?;
    let browse = browse(&discovery)?;
    status.set_discovery_active(true);
    Some(LanSetup {
        endpoint,
        discovery,
        browse,
        ctx: LanCtx { ws, device, group },
    })
}

async fn run(ws: SharedWorkspace, clock: Arc<dyn Clock>) {
    let Some(LanSetup {
        endpoint,
        discovery: _discovery,
        browse,
        ctx,
    }) = setup(ws).await
    else {
        return;
    };
    let mut table = PeerTable::new(ctx.device, ctx.group);
    let dial_state = Arc::new(std::sync::Mutex::new(DialState::default()));
    let sessions = Arc::new(Semaphore::new(MAX_CONCURRENT_LAN_SESSIONS));

    loop {
        tokio::select! {
            incoming = endpoint.accept() => accept_one(incoming, &sessions, &ctx),
            sighting = browse.recv() => {
                let keep_going = handle_sighting(
                    sighting, &mut table, &dial_state, clock.as_ref(), &sessions, &ctx, &endpoint,
                );
                if !keep_going {
                    return;
                }
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn handle_sighting(
    sighting: Option<Sighting>,
    table: &mut PeerTable,
    dial_state: &Arc<std::sync::Mutex<DialState>>,
    clock: &dyn Clock,
    sessions: &Arc<Semaphore>,
    ctx: &LanCtx,
    endpoint: &Arc<LanEndpoint>,
) -> bool {
    let Some(sighting) = sighting else {
        tracing::warn!("lan_discovery_channel_closed");
        return false;
    };
    let now_ms = clock.now_ms();
    if let Some(peer) = worth_dialing(sighting, table, dial_state, now_ms, ctx.device) {
        spawn_dial(
            Arc::clone(sessions),
            ctx.clone(),
            Arc::clone(endpoint),
            Arc::clone(dial_state),
            peer,
        );
    }
    true
}

async fn bind_endpoint() -> Option<LanEndpoint> {
    match LanEndpoint::bind().await {
        Ok(e) => Some(e),
        Err(e) => {
            tracing::warn!(error = %e, "lan_bind_failed_running_without_lan_sync");
            None
        }
    }
}

fn start_discovery(device: DeviceId, group: GroupId, endpoint: &LanEndpoint) -> Option<Discovery> {
    let host_name = format!("{device}.local.");
    let port = endpoint.advertise_port().unwrap_or(0);
    match Discovery::start(device, group, endpoint.node_id_bytes(), &host_name, port) {
        Ok(d) => Some(d),
        Err(e) => {
            tracing::warn!(error = %e, "lan_discovery_failed_running_without_lan_sync");
            None
        }
    }
}

fn browse(discovery: &Discovery) -> Option<txtodo_sync::BrowseEvents> {
    match discovery.browse() {
        Ok(b) => Some(b),
        Err(e) => {
            tracing::warn!(error = %e, "lan_browse_failed_running_without_lan_sync");
            None
        }
    }
}

fn spawn_driver(
    ws: SharedWorkspace,
    device: DeviceId,
    group: GroupId,
    link: IrohLink,
    permit: tokio::sync::OwnedSemaphorePermit,
) {
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        let mut link = link;
        drive_session(&mut link, ws, device, group);
    });
}

fn log_accept_failed(e: &txtodo_sync::LanError) {
    tracing::debug!(error = %e, "lan_accept_failed");
}

fn log_session_cap_reached_dropping_incoming() {
    tracing::warn!(
        cap = MAX_CONCURRENT_LAN_SESSIONS,
        "lan_session_cap_reached_dropping_incoming"
    );
}

/// One accepted connection: spawns a driver if the session cap allows it, otherwise the link is
/// simply dropped (closing it) and logged — the same "bounded, drop with a reason" shape
/// `MAX_LAN_PEERS` already uses for the peer table.
fn accept_one(
    incoming: Result<IrohLink, txtodo_sync::LanError>,
    sessions: &Arc<Semaphore>,
    ctx: &LanCtx,
) {
    let Ok(link) = incoming.inspect_err(log_accept_failed) else {
        return;
    };
    let Ok(permit) = Arc::clone(sessions).try_acquire_owned() else {
        log_session_cap_reached_dropping_incoming();
        return;
    };
    spawn_driver(ctx.ws.clone(), ctx.device, ctx.group, link, permit);
}

/// `table.observe` plus the dial tie-break and backoff check, collapsed to one `Option`: `Some`
/// only when this device should actually dial `peer` right now.
fn worth_dialing(
    sighting: Sighting,
    table: &mut PeerTable,
    dial_state: &Arc<std::sync::Mutex<DialState>>,
    now_ms: u64,
    device: DeviceId,
) -> Option<DiscoveredPeer> {
    let PeerEvent::Found(peer) = table.observe(sighting.announcement, sighting.addresses, now_ms)
    else {
        return None;
    };
    log_peer_found(&peer);
    let mut dial_state = dial_state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    // Tie-break: only the lower device id dials, so two daemons that discover each other at the
    // same moment never open two redundant connections.
    if device >= peer.device || !dial_state.due(peer.device, now_ms) {
        return None;
    }
    dial_state.record_attempt(peer.device, now_ms);
    Some(peer)
}

/// Logged for every real sighting, dialed or not — the only externally observable (via the JSON
/// log) proof that discovery itself worked, independent of whether the connect step that follows
/// succeeds (`lan.rs`'s module doc on the confirmed same-host connect blocker).
fn log_peer_found(peer: &DiscoveredPeer) {
    tracing::info!(peer = %peer.device, addresses = ?peer.addresses, "lan_peer_found");
}

fn record_dial_outcome(dial_state: &Arc<std::sync::Mutex<DialState>>, peer: DeviceId, ok: bool) {
    let mut dial_state = dial_state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if ok {
        dial_state.record_success(peer);
    } else {
        dial_state.record_failure(peer);
    }
}

fn spawn_dial(
    sessions: Arc<Semaphore>,
    ctx: LanCtx,
    endpoint: Arc<LanEndpoint>,
    dial_state: Arc<std::sync::Mutex<DialState>>,
    peer: DiscoveredPeer,
) {
    let Ok(permit) = sessions.try_acquire_owned() else {
        tracing::debug!(peer = %peer.device, "lan_session_cap_reached_skipping_dial");
        return;
    };
    tokio::spawn(async move {
        let dial = endpoint.connect(peer.node, &peer.addresses);
        match tokio::time::timeout(CONNECT_TIMEOUT, dial).await {
            Ok(Ok(link)) => {
                record_dial_outcome(&dial_state, peer.device, true);
                spawn_driver(ctx.ws, ctx.device, ctx.group, link, permit);
            }
            Ok(Err(e)) => {
                record_dial_outcome(&dial_state, peer.device, false);
                log_connect_failed(peer.device, &e);
            }
            Err(_) => {
                record_dial_outcome(&dial_state, peer.device, false);
                tracing::debug!(peer = %peer.device, "lan_connect_timed_out");
            }
        }
    });
}

fn log_connect_failed(peer: DeviceId, e: &txtodo_sync::LanError) {
    tracing::debug!(peer = %peer, error = %e, "lan_connect_failed");
}
