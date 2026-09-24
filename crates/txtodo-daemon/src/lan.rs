//! Wires `txtodo-sync`'s `Discovery` and `LanEndpoint` into a running `txtodod` (plan M4
//! `sync-lan-transport`): finds other daemons for the same sync group on the LAN and drives a
//! `Session` over the real transport with each one. Never fatal to the daemon — a bind or
//! discovery failure is logged and this device simply runs without LAN sync. `iroh`/`mdns-sd`
//! never appear here or anywhere else in this crate; only `txtodo_sync`'s own types do.
//!
//! **One per device** (task `sync-live-push`, 2026-09-24): bound once in `main.rs` next to
//! `DeviceRelay`, advertised once, and every connection is driven by
//! `lan_session_dispatch::drive_shared_session` over [`DeviceLan`]'s route table, so one LAN link
//! carries every open workspace, the same as the relay path. It used to be one endpoint, one mDNS
//! advertisement and one link per open workspace.
//!
//! **Sessions are long-lived** (task `sync-live-push`): once two devices share a workspace, the
//! connection stays open, commits are pushed over it and an empty `Ack` heartbeat keeps it up
//! (`lan_session_live.rs`). The periodic resync below (`relay_autodial::spawn_resync_dial`) is now
//! a reconnect: it skips every peer with a live session (`live_peers.rs`) and dials the rest every
//! `RESYNC_INTERVAL`. With no shared workspace a session still ends after the old 750 ms of quiet.
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
use txtodo_sync::{DiscoveredPeer, Discovery, GroupId, IrohLink, LanEndpoint, Sighting};

use crate::clock::Clock;
use crate::device_identity::DeviceIdentity;
use crate::device_lan::DeviceLan;
use crate::device_relay::DeviceRelay;
use crate::lan_peers::{
    DialState, KnownPeers, SharedDialState, known_or, record_dial_outcome, remember_any_sighting,
    remember_sighting, worth_dialing,
};
use crate::live_peers::Carrier;

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

/// Starts LAN discovery and the iroh endpoint as a background task, once per device.
pub(crate) fn start(ctx: LanCtx) -> LanTransport {
    LanTransport {
        task: tokio::spawn(run(ctx)),
    }
}

/// Everything the device's one LAN task shares with its dials and sessions — bundled to stay under
/// `maxParams`, cheap to clone. Fields are `pub(crate)`: `relay_fallback.rs`/`relay_autodial.rs`
/// need them too. `group` is the group `Discovery` advertises; a pairing changes it (see
/// `rebuild_on_group_change`).
#[derive(Clone)]
pub(crate) struct LanCtx {
    pub(crate) identity: Arc<DeviceIdentity>,
    pub(crate) lan: Arc<DeviceLan>,
    pub(crate) device_relay: Option<Arc<DeviceRelay>>,
    pub(crate) clock: Arc<dyn Clock>,
    pub(crate) device: DeviceId,
    pub(crate) group: GroupId,
    /// Where LAN control sessions read this device's offers from; `None` means no offers over LAN.
    pub(crate) registry:
        Option<Arc<std::sync::Mutex<crate::workspace_registry::WorkspaceRegistry>>>,
}

/// Everything a bind/discover/browse setup produces, kept alive for the run loop's whole life.
struct LanSetup {
    endpoint: Arc<LanEndpoint>,
    discovery: Discovery,
    browse: txtodo_sync::BrowseEvents,
    ctx: LanCtx,
}

async fn setup(mut ctx: LanCtx) -> Option<LanSetup> {
    let endpoint = Arc::new(bind_endpoint().await?);
    ctx.identity
        .pairing_lan()
        .set_endpoint(Arc::clone(&endpoint));
    let status = ctx.identity.lan_status().clone();
    status.set_endpoint_bound(true);
    ctx.group = ctx.identity.group();
    let discovery = start_discovery(ctx.device, ctx.group, &endpoint)?;
    let browse = browse(&discovery)?;
    status.set_discovery_active(true);
    Some(LanSetup {
        endpoint,
        discovery,
        browse,
        ctx,
    })
}

async fn run(ctx: LanCtx) {
    let Some(LanSetup {
        endpoint,
        mut discovery,
        mut browse,
        mut ctx,
    }) = setup(ctx).await
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
            incoming = endpoint.accept() => {
                crate::device_lan::accept_one(incoming, &sessions, &ctx);
            }
            sighting = browse.recv() => {
                let keep_going = handle_sighting(
                    sighting, &mut table, &dial_state, &known_peers, &sessions, &ctx, &endpoint,
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
                crate::device_lan::dial_control(&known_peers, &ctx, &endpoint, &sessions);
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
    let current = ctx.identity.group();
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
    remember_any_sighting(ctx.identity.pairing_lan(), &sighting);
    remember_sighting(known_peers, &sighting, ctx.device, ctx.group);
    let now_ms = ctx.clock.now_ms();
    if let Some(peer) = worth_dialing(sighting, table, dial_state, now_ms, ctx.device) {
        let peer = known_or(known_peers, peer);
        // A live LAN session already carries every shared workspace (task sync-live-push). One
        // live only over the relay is dialed anyway: the LAN session supersedes it.
        if ctx
            .identity
            .live_peers()
            .is_live_on(peer.device, Carrier::Lan)
        {
            return true;
        }
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
    (link, carrier): (IrohLink, Carrier),
    permit: tokio::sync::OwnedSemaphorePermit,
    on_done: impl FnOnce(bool) + Send + 'static,
) {
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        let mut link = link;
        let greeted = crate::lan_session_dispatch::drive_shared_session(
            &mut link,
            ctx.lan.routes(),
            ctx.device,
            ctx.group,
            carrier,
        );
        on_done(greeted);
    });
}

/// At `info` (task lan-dial-falls-to-relay): the one line that says why a LAN peer ended up on
/// the relay, with the addresses tried.
fn log_connect_failed(peer: &DiscoveredPeer, e: &txtodo_sync::LanError) {
    tracing::info!(peer = %peer.device, addresses = ?peer.addresses, error = %e, "lan_connect_failed");
}

/// The LAN half of the fallback: `None` on any failure, already logged via `log_connect_failed`.
async fn lan_only_dial(endpoint: Arc<LanEndpoint>, peer: DiscoveredPeer) -> Option<IrohLink> {
    match endpoint.connect(peer.node, &peer.addresses).await {
        Ok(link) => Some(link),
        Err(e) => {
            log_connect_failed(&peer, &e);
            None
        }
    }
}

/// Connects to `peer`, spawning its session driver on success (`permit` drops either way). Tries
/// LAN first, falling back to relay (`crate::relay_fallback`) only when LAN doesn't produce a link
/// within `CONNECT_TIMEOUT` — ADR 0026: LAN stays primary, relay is additive. Returns whether a
/// link was established; the dial's `DialState` outcome is booked here, on the driver thread once
/// the session ends (a connect that bails before greeting is a failure, so `backoff_ms` applies).
/// A peer already live (over the relay: callers skip one live over LAN) gets no relay fallback:
/// this dial is the LAN upgrade (task lan-dial-falls-to-relay), and a second relay session would
/// only duplicate the first. `pub(crate)`: `relay_autodial.rs`'s resync dial is the same.
pub(crate) async fn dial_and_spawn(
    ctx: LanCtx,
    endpoint: Arc<LanEndpoint>,
    peer: DiscoveredPeer,
    permit: tokio::sync::OwnedSemaphorePermit,
    dial_state: SharedDialState,
) -> bool {
    let node = peer.node;
    let device = peer.device;
    let upgrade = ctx.identity.live_peers().is_live(device);
    let lan_dial = async { Some((lan_only_dial(endpoint, peer).await?, Carrier::Lan)) };
    let relay_ctx = ctx.clone();
    let relay_dial = async move {
        if upgrade {
            return None;
        }
        let link = crate::relay_fallback::relay_fallback_dial(relay_ctx, node).await?;
        Some((link, Carrier::Relay))
    };
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
