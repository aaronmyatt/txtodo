//! Wires `txtodo-sync`'s `Discovery` and `LanEndpoint` into a running `txtodod` (plan M4
//! `sync-lan-transport`): finds other daemons for the same sync group on the LAN and drives a
//! `Session` over the real transport with each one. Never fatal to the daemon — a bind or
//! discovery failure is logged and this device simply runs without LAN sync, the same as a fresh
//! workspace with no group key yet. `iroh`/`mdns-sd` never appear here or anywhere else in this
//! crate; only `txtodo_sync`'s own types do (`.claude/budgets.json`'s `allowedDeps`).
//!
//! **Sessions are short-lived by design.** `IrohLink::recv` (`txtodo-sync`) reports the link
//! closed after `IDLE_TIMEOUT` (750 ms) of silence, not only on a real close, so
//! `lan_session::drive_session` naturally returns once a connection has caught the peer up and
//! gone quiet. The periodic resync below (`spawn_resync_dial`, driven from `run`'s own timer) is
//! the other half: every known peer is redialed every `RESYNC_INTERVAL`, so a local edit made
//! after an earlier round still converges quickly, without this module needing to watch the store
//! for changes. The cost — a new QUIC handshake roughly every second for as long as two daemons
//! stay paired and on the LAN — is a known, flagged tradeoff of this M4-scoped design; a
//! push/notify model would avoid it.
//!
//! **Real same-host, cross-process connect works.** An earlier pass of this task believed a real
//! `iroh` QUIC connect could never complete between two endpoints on the same host at all — true
//! only when both endpoints live in the *same process* (confirmed with hard evidence in
//! `txtodo-sync`'s `endpoint_tests.rs`). Two real `txtodod` *processes* on one machine connect and
//! sync for real: `tests/lan_loopback_converge.rs` measures real, repeatable sub-2-second
//! convergence in both directions. `LanEndpoint::connect` still prefers non-loopback addresses
//! (falling back to loopback only when nothing else was advertised) since a real LAN would never
//! offer only loopback in the first place — see `txtodo-sync`'s `CLAUDE.md`.

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::Semaphore;
use tokio::task::JoinHandle;
use txtodo_model::DeviceId;
use txtodo_sync::{
    DiscoveredPeer, Discovery, GroupId, IrohLink, LanEndpoint, PAIRING_ALPN, Sighting,
};

use crate::clock::Clock;
use crate::lan_peers::{
    DialState, KnownPeers, SharedDialState, peers_to_resync, record_dial_outcome, remember_peer,
    worth_dialing,
};
use crate::lan_session::{drive_session, read};
use crate::server::SharedWorkspace;

/// Refuses a 101st concurrent sync session the same way `MAX_LAN_PEERS` bounds the peer table
/// itself — a LAN flooded with peers must not spawn unbounded tasks.
pub const MAX_CONCURRENT_LAN_SESSIONS: usize = 16;

/// Bounded: a real connect that never resolves (a black-holed peer) cannot hang this forever.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// How often already-known peers are redialed — see the module doc on why this, together with
/// `IrohLink`'s `IDLE_TIMEOUT`, is what keeps sync "live". Longer than `IDLE_TIMEOUT` so the
/// common case is one live session per peer, not two briefly overlapping ones.
const RESYNC_INTERVAL: Duration = Duration::from_millis(1_000);

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
        ws.pairing_lan().set_endpoint(Arc::clone(&endpoint));
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
    let mut table = txtodo_sync::PeerTable::new(ctx.device, ctx.group);
    let dial_state: SharedDialState = Arc::new(std::sync::Mutex::new(DialState::default()));
    let known_peers: KnownPeers =
        Arc::new(std::sync::Mutex::new(std::collections::BTreeMap::new()));
    let sessions = Arc::new(Semaphore::new(MAX_CONCURRENT_LAN_SESSIONS));
    let mut resync = tokio::time::interval(RESYNC_INTERVAL);
    resync.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    loop {
        tokio::select! {
            incoming = endpoint.accept() => accept_one(incoming, &sessions, &ctx),
            sighting = browse.recv() => {
                let keep_going = handle_sighting(
                    sighting, &mut table, &dial_state, &known_peers, clock.as_ref(), &sessions,
                    &ctx, &endpoint,
                );
                if !keep_going {
                    return;
                }
            }
            _ = resync.tick() => {
                for peer in peers_to_resync(&known_peers, ctx.device) {
                    spawn_resync_dial(Arc::clone(&sessions), ctx.clone(), Arc::clone(&endpoint), peer);
                }
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn handle_sighting(
    sighting: Option<Sighting>,
    table: &mut txtodo_sync::PeerTable,
    dial_state: &SharedDialState,
    known_peers: &KnownPeers,
    clock: &dyn Clock,
    sessions: &Arc<Semaphore>,
    ctx: &LanCtx,
    endpoint: &Arc<LanEndpoint>,
) -> bool {
    let Some(sighting) = sighting else {
        tracing::warn!("lan_discovery_channel_closed");
        return false;
    };
    // Remembered regardless of group, *before* the group-filtered dial decision below: pairing
    // (`pairing_lan.rs`) looks a peer up by device id alone, since the whole point of pairing is
    // that the two devices do not share a group yet (see `pairing_lan_state.rs`'s module doc).
    remember_any_sighting(&ctx.ws, &sighting);
    let now_ms = clock.now_ms();
    if let Some(peer) = worth_dialing(sighting, table, dial_state, now_ms, ctx.device) {
        remember_peer(known_peers, &peer);
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

/// Records `sighting` in `pairing_lan()`'s unfiltered address book, regardless of which group it
/// claims — see `handle_sighting`'s call site and `pairing_lan_state.rs`'s module doc.
fn remember_any_sighting(ws: &SharedWorkspace, sighting: &Sighting) {
    let peer = DiscoveredPeer {
        device: sighting.announcement.device,
        node: sighting.announcement.node,
        addresses: sighting.addresses.clone(),
    };
    read(ws).pairing_lan().remember(&peer);
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

/// One accepted pairing connection (this device as initiator) — same "blocking thread, one permit"
/// shape as [`spawn_driver`], since `Link::send`/`recv` block (`lan_link.rs`'s own doc). Unlike a
/// sync session, a pairing connection needs no `device`/`group` of its own: `pairing_lan.rs`'s
/// handler reads whatever this daemon's own active `PairingRegistry` session says.
fn spawn_pairing_driver(
    ws: SharedWorkspace,
    link: IrohLink,
    permit: tokio::sync::OwnedSemaphorePermit,
) {
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        let mut link = link;
        crate::pairing_lan::handle_incoming(&ws, &mut link);
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
/// `MAX_LAN_PEERS` already uses for the peer table. Which driver depends on `link.alpn()`: the one
/// bound endpoint accepts both the group-keyed sync protocol and a pairing relay connection (plan
/// M4 `sync-pairing`'s LAN wiring pass), told apart here rather than by any frame content.
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
    if link.alpn() == PAIRING_ALPN {
        spawn_pairing_driver(ctx.ws.clone(), link, permit);
    } else {
        spawn_driver(ctx.ws.clone(), ctx.device, ctx.group, link, permit);
    }
}

fn log_connect_failed(peer: DeviceId, e: &txtodo_sync::LanError) {
    tracing::debug!(peer = %peer, error = %e, "lan_connect_failed");
}

/// Connects to `peer` and, on success, spawns its session driver; `permit` is dropped on every
/// path either way (held onward by the driver only after a successful connect).
async fn dial_and_spawn(
    ctx: LanCtx,
    endpoint: Arc<LanEndpoint>,
    peer: DiscoveredPeer,
    permit: tokio::sync::OwnedSemaphorePermit,
) -> bool {
    let dial = endpoint.connect(peer.node, &peer.addresses);
    match tokio::time::timeout(CONNECT_TIMEOUT, dial).await {
        Ok(Ok(link)) => {
            spawn_driver(ctx.ws, ctx.device, ctx.group, link, permit);
            true
        }
        Ok(Err(e)) => {
            log_connect_failed(peer.device, &e);
            false
        }
        Err(_) => {
            tracing::debug!(peer = %peer.device, "lan_connect_timed_out");
            false
        }
    }
}

fn spawn_dial(
    sessions: Arc<Semaphore>,
    ctx: LanCtx,
    endpoint: Arc<LanEndpoint>,
    dial_state: SharedDialState,
    peer: DiscoveredPeer,
) {
    let Ok(permit) = sessions.try_acquire_owned() else {
        tracing::debug!(peer = %peer.device, "lan_session_cap_reached_skipping_dial");
        return;
    };
    tokio::spawn(async move {
        let device = peer.device;
        let ok = dial_and_spawn(ctx, endpoint, peer, permit).await;
        record_dial_outcome(&dial_state, device, ok);
    });
}

/// The periodic-resync counterpart of `spawn_dial`: same connect-and-drive, but no `DialState`
/// bookkeeping — deliberate, unconditional churn rather than failure recovery (module doc).
fn spawn_resync_dial(
    sessions: Arc<Semaphore>,
    ctx: LanCtx,
    endpoint: Arc<LanEndpoint>,
    peer: DiscoveredPeer,
) {
    let Ok(permit) = sessions.try_acquire_owned() else {
        tracing::debug!(peer = %peer.device, "lan_session_cap_reached_skipping_resync");
        return;
    };
    tokio::spawn(async move {
        let _ = dial_and_spawn(ctx, endpoint, peer, permit).await;
    });
}
