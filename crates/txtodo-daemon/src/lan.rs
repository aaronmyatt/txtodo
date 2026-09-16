//! Wires `txtodo-sync`'s `Discovery` and `LanEndpoint` into a running `txtodod` (plan M4
//! `sync-lan-transport`): finds other daemons for the same sync group on the LAN and drives a
//! `Session` over the real transport with each one. Never fatal to the daemon — a bind or
//! discovery failure is logged and this device simply runs without LAN sync. `iroh`/`mdns-sd`
//! never appear here or anywhere else in this crate; only `txtodo_sync`'s own types do.
//!
//! **Sessions are short-lived by design.** `IrohLink::recv` (`txtodo-sync`) reports the link
//! closed after `IDLE_TIMEOUT` (750 ms) of silence, so `lan_session::drive_session` naturally
//! returns once a connection has caught the peer up and gone quiet. The periodic resync below
//! (`spawn_resync_dial`) is the other half: every known peer is redialed every `RESYNC_INTERVAL`,
//! so a later local edit still converges quickly without this module watching the store — a new
//! QUIC handshake roughly every second while paired is a known, flagged tradeoff.
//!
//! **Real same-host, cross-process connect works.** A real `iroh` QUIC connect only ever fails
//! between two endpoints in the *same process*; two real `txtodod` processes on one host connect
//! and sync for real (`tests/lan_loopback_converge.rs`). `LanEndpoint::connect` prefers
//! non-loopback addresses, loopback only as a last resort.

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
/// `pub(crate)`: `relay_fallback.rs`'s relay dial reuses the same bound.
pub(crate) const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// How often already-known peers are redialed — see the module doc on why this, together with
/// `IrohLink`'s `IDLE_TIMEOUT`, is what keeps sync "live". Longer than `IDLE_TIMEOUT` so the
/// common case is one live session per peer, not two briefly overlapping ones.
const RESYNC_INTERVAL: Duration = Duration::from_millis(1_000);

/// The background LAN transport task; `abort()` on daemon shutdown.
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

/// This device's identity for the sync group, plus the workspace — bundled to stay under
/// `maxParams`, cheap to clone. Fields are `pub(crate)`: `relay_fallback.rs` needs them too.
#[derive(Clone)]
pub(crate) struct LanCtx {
    pub(crate) ws: SharedWorkspace,
    pub(crate) device: DeviceId,
    pub(crate) group: GroupId,
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
        mut discovery,
        mut browse,
        mut ctx,
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
                if let Some(rebuilt) = rebuild_on_group_change(&ctx, &endpoint).await {
                    discovery.shutdown();
                    discovery = rebuilt.discovery;
                    browse = rebuilt.browse;
                    ctx.group = rebuilt.group;
                    table = txtodo_sync::PeerTable::new(ctx.device, ctx.group);
                }
                for peer in peers_to_resync(&known_peers, ctx.device) {
                    spawn_resync_dial(Arc::clone(&sessions), ctx.clone(), Arc::clone(&endpoint), peer);
                }
            }
        }
    }
}

/// What `rebuild_on_group_change` produces when the workspace's group changed.
struct Rebuilt {
    discovery: Discovery,
    browse: txtodo_sync::BrowseEvents,
    group: GroupId,
}

/// Pairing can change this workspace's own sync group *after* `setup()` already bound `Discovery`
/// to the old one, which neither notices on its own. Checked once per `RESYNC_INTERVAL` tick:
/// re-advertises under the current group and returns a fresh `Discovery`/browse stream for `run`'s
/// loop to swap in, or `None` when the group has not changed since `ctx.group`.
async fn rebuild_on_group_change(ctx: &LanCtx, endpoint: &LanEndpoint) -> Option<Rebuilt> {
    let current = read(&ctx.ws).group();
    if current == ctx.group {
        return None;
    }
    let discovery = start_discovery(ctx.device, current, endpoint)?;
    let browse = browse(&discovery)?;
    tracing::info!(old = ?ctx.group, new = ?current, "lan_group_changed_readvertising");
    Some(Rebuilt {
        discovery,
        browse,
        group: current,
    })
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

/// `pub(crate)`: `relay.rs` reuses this too — `RelayEndpoint` returns the same `IrohLink` type.
pub(crate) fn spawn_driver(
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
/// shape as [`spawn_driver`]. Unlike a sync session, this needs no `device`/`group` of its own:
/// `pairing_lan.rs`'s handler reads whatever this daemon's own active `PairingRegistry` says.
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
/// simply dropped (closing it) and logged. Which driver depends on `link.alpn()`: the one bound
/// endpoint accepts both the group-keyed sync protocol and a pairing relay connection, told apart
/// here rather than by any frame content.
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

/// The LAN half of the fallback: `None` on any failure, already logged via `log_connect_failed`.
async fn lan_only_dial(endpoint: Arc<LanEndpoint>, peer: DiscoveredPeer) -> Option<IrohLink> {
    match endpoint.connect(peer.node, &peer.addresses).await {
        Ok(link) => Some(link),
        Err(e) => {
            log_connect_failed(peer.device, &e);
            None
        }
    }
}

/// Connects to `peer`, spawning its session driver on success (`permit` drops either way). Tries
/// LAN first, falling back to relay (`crate::relay_fallback`) only when LAN doesn't produce a link
/// within `CONNECT_TIMEOUT` — ADR 0026: LAN stays primary, relay is additive.
async fn dial_and_spawn(
    ctx: LanCtx,
    endpoint: Arc<LanEndpoint>,
    peer: DiscoveredPeer,
    permit: tokio::sync::OwnedSemaphorePermit,
) -> bool {
    let node = peer.node;
    let device = peer.device;
    let lan_dial = lan_only_dial(endpoint, peer);
    let relay_dial = crate::relay_fallback::relay_fallback_dial(ctx.clone(), node);
    match crate::relay_fallback::lan_then_relay(CONNECT_TIMEOUT, lan_dial, relay_dial).await {
        Some(link) => {
            spawn_driver(ctx.ws, ctx.device, ctx.group, link, permit);
            true
        }
        None => {
            tracing::debug!(peer = %device, "lan_and_relay_dial_both_failed");
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
        let device = peer.device;
        let ok = dial_and_spawn(ctx, endpoint, peer, permit).await;
        log_resync_dial_outcome(device, ok);
    });
}

/// Previously discarded outright (`let _ = dial_and_spawn(...).await;`) — no `DialState`
/// bookkeeping added here on purpose (module doc: unconditional churn, not failure recovery), just
/// visibility that a periodic resync dial happened and how it went.
fn log_resync_dial_outcome(peer: DeviceId, ok: bool) {
    tracing::debug!(%peer, ok, "lan_resync_dial_outcome");
}
