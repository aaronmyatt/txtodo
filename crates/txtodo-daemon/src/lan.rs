//! Wires `txtodo-sync`'s `Discovery` and `LanEndpoint` into a running `txtodod` (plan M4
//! `sync-lan-transport`): finds other daemons for the same sync group on the LAN and drives a
//! `Session` over the real transport with each one. Never fatal to the daemon — a bind or
//! discovery failure is logged and this device simply runs without LAN sync. `iroh`/`mdns-sd`
//! never appear here or anywhere else in this crate; only `txtodo_sync`'s own types do.
//!
//! **Sessions are short-lived by design.** `IrohLink::recv` (`txtodo-sync`) reports the link
//! closed after `IDLE_TIMEOUT` (750 ms) of silence, so `lan_session::drive_session` naturally
//! returns once a connection has caught the peer up and gone quiet. The periodic resync below
//! (`relay_autodial::spawn_resync_dial`) is the other half: every known peer is redialed every
//! `RESYNC_INTERVAL`, so a later local edit still converges without this module watching the store.
//! Resync dials share `DialState`'s backoff with sighting dials, and a session that connects but
//! bails before its first greeting (no group key yet, no routed workspace) counts as a failure —
//! measured 2026-09-23 at ~3 sessions a second between two unpaired daemons, each one a keystore
//! read, before either bound was in place.
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
    DialState, KnownPeers, SharedDialState, record_dial_outcome, remember_any_sighting,
    remember_peer, worth_dialing,
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
/// `IrohLink`'s `IDLE_TIMEOUT`, is what keeps sync "live". Was 1 s until 2026-09-23: two daemons
/// each redialing the other every second ran ~3 sessions a second (1164 in 7 min on one Mac), each
/// one a keystore read. 15 s still converges a local edit within a human's patience.
const RESYNC_INTERVAL: Duration = Duration::from_secs(15);

/// Overrides `RESYNC_INTERVAL`, in milliseconds. A test seam like `debug_hooks.rs`'s
/// `TXTODO_TEST_HOOKS`: `tests/lan_loopback_converge.rs` asserts a second-direction edit lands
/// within 2 s, which only a redial delivers, so the harnesses set this to 1000.
pub const RESYNC_INTERVAL_ENV_VAR: &str = "TXTODO_RESYNC_INTERVAL_MS";

/// How often `rebuild_on_group_change` looks for a pairing having changed this workspace's group:
/// one cheap read, kept at 1 s when `RESYNC_INTERVAL` grew to 15 s — a freshly paired joiner is
/// invisible to its peer until it re-advertises under the new group.
const GROUP_CHECK_INTERVAL: Duration = Duration::from_secs(1);

/// `pub(crate)`: `relay.rs`'s and `control_channel.rs`'s dial loops share this cadence and knob.
pub(crate) fn resync_interval() -> Duration {
    std::env::var(RESYNC_INTERVAL_ENV_VAR)
        .ok()
        .and_then(|v| v.parse().ok())
        .map_or(RESYNC_INTERVAL, Duration::from_millis)
}

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
    let mut group_check = tokio::time::interval(GROUP_CHECK_INTERVAL);
    group_check.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut resync = tokio::time::interval(resync_interval());
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
            _ = group_check.tick() => {
                if let Some(rebuilt) = rebuild_on_group_change(&ctx, &endpoint).await {
                    discovery.shutdown();
                    discovery = rebuilt.discovery;
                    browse = rebuilt.browse;
                    ctx.group = rebuilt.group;
                    table = txtodo_sync::PeerTable::new(ctx.device, ctx.group);
                }
            }
            _ = resync.tick() => {
                crate::relay_autodial::resync_and_dial(
                    &known_peers,
                    &ctx,
                    &endpoint,
                    &sessions,
                    &dial_state,
                );
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

/// `pub(crate)`: `relay_autodial.rs` reuses this too — `RelayEndpoint` returns the same
/// `IrohLink` type. `on_done(greeted)` runs on the driver thread once the session ends; `greeted`
/// is false when it bailed before its first greeting, which the dial path books as a failure.
pub(crate) fn spawn_driver(
    ctx: LanCtx,
    link: IrohLink,
    permit: tokio::sync::OwnedSemaphorePermit,
    on_done: impl FnOnce(bool) + Send + 'static,
) {
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        let mut link = link;
        let greeted = drive_session(&mut link, ctx.ws, ctx.device, ctx.group);
        on_done(greeted);
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
        spawn_driver(ctx.clone(), link, permit, |_| {});
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
/// within `CONNECT_TIMEOUT` — ADR 0026: LAN stays primary, relay is additive. Returns whether a
/// link was established; the dial's `DialState` outcome is booked here, on the driver thread once
/// the session ends (a connect that bails before greeting is a failure, so `backoff_ms` applies).
/// `pub(crate)`: `relay_autodial.rs`'s resync dial is the same connect-and-drive.
pub(crate) async fn dial_and_spawn(
    ctx: LanCtx,
    endpoint: Arc<LanEndpoint>,
    peer: DiscoveredPeer,
    permit: tokio::sync::OwnedSemaphorePermit,
    dial_state: SharedDialState,
) -> bool {
    let node = peer.node;
    let device = peer.device;
    let lan_dial = lan_only_dial(endpoint, peer);
    let relay_dial = crate::relay_fallback::relay_fallback_dial(ctx.clone(), node);
    match crate::relay_fallback::lan_then_relay(CONNECT_TIMEOUT, lan_dial, relay_dial).await {
        Some(link) => {
            spawn_driver(ctx, link, permit, move |greeted| {
                record_dial_outcome(&dial_state, device, greeted);
            });
            true
        }
        None => {
            tracing::debug!(peer = %device, "lan_and_relay_dial_both_failed");
            record_dial_outcome(&dial_state, device, false);
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
        dial_and_spawn(ctx, endpoint, peer, permit, dial_state).await;
    });
}
