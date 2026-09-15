//! Wires `txtodo_sync::RelayEndpoint` into a running `txtodod` (plan M8 `sync-relay-enable`,
//! ADR 0026): binds the relay-configured endpoint when `--relay <url>` is set and runs an accept
//! loop, updating `LanStatus`'s relay fields on every bind/accept outcome. Twin of `lan.rs`, but
//! relay is optional and, per ADR 0026, purely additive — LAN stays the primary path. This module
//! only binds the endpoint and accepts incoming relay connections; the *dialing* half of the
//! fallback (this device reaching a peer over relay when LAN can't) is `lan.rs::dial_and_spawn`,
//! via `relay_fallback.rs` and `RelayState` — for a peer LAN *discovered* but could not reach.
//!
//! **`--relay-dial-peer` (plan M8 `relay-converge-test`): the rendezvous gap that pass left open.**
//! `relay_fallback_dial`'s own doc names a real limitation — it dials a peer's *LAN* node id over
//! relay, reachable only once both carriers share one identity, not built yet. Investigating it for
//! this task surfaced a second, deeper gap: `lan.rs::dial_and_spawn` (and therefore
//! `relay_fallback_dial`) only ever runs for a peer `handle_sighting` already learned about via
//! **mDNS**, which by construction never crosses a real network boundary — two daemons that were
//! never on the same LAN never populate each other's `PeerTable` at all, so the relay fallback path
//! is simply never reached for them, identity-sharing aside. Real pairing-over-relay (a rendezvous
//! protocol that works with no shared LAN) is `sync-pairing-relay`'s own not-yet-built task per ADR
//! 0026's follow-up list — out of scope here to build in full. `--relay-dial-peer <hex node id>`
//! is this task's minimal, honestly-scoped substitute: a caller who already knows a peer's *relay*
//! node id (e.g. read off that peer's own `Health.relay_last_outcome`, or a test harness that
//! seeded it) can hand it to this daemon at startup, and [`dial_known_peer`] below drives the
//! connect/sync loop directly over the bound `RelayEndpoint` — no LAN discovery, no shared LAN,
//! ever required. This sidesteps the identity-sharing gap too: it dials the peer's *actual* relay
//! identity, never conflates it with a LAN one.
//!
//! **Known scope limit, deliberate**: unlike `lan.rs::rebuild_on_group_change`, this module never
//! rebinds anything when the workspace's sync group changes (e.g. mid-run pairing). Pairing-over-
//! relay is a separate, not-yet-built task (`sync-pairing-relay`, scoped by ADR 0026's own
//! follow-up list) — out of scope here. A relay endpoint bound before a pairing completes will
//! simply keep gating `connect`/`accept` on the group it was bound with.

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::Semaphore;
use tokio::task::JoinHandle;
use txtodo_model::DeviceId;
use txtodo_sync::{
    GroupId, HolepunchError, MAX_RELAY_PEERS, PAIRING_ALPN, RelayConfig, RelayEndpoint,
};

use crate::lan::{MAX_CONCURRENT_LAN_SESSIONS, spawn_driver};
use crate::lan_session::read;
use crate::lan_status::LanStatus;
use crate::server::SharedWorkspace;

/// How often [`dial_known_peer`] retries `--relay-dial-peer` while it has not yet connected, and
/// (once it has) redials to pick up a local edit made after the previous sync round — the relay
/// counterpart of `lan.rs`'s `RESYNC_INTERVAL`, same reasoning.
const DIAL_KNOWN_PEER_INTERVAL: Duration = Duration::from_millis(1_000);

/// This device's identity plus its workspace and live status — bundled the same way `lan.rs`'s own
/// `LanCtx` is, so no function below needs more than `maxParams` arguments.
#[derive(Clone)]
struct RelayCtx {
    ws: SharedWorkspace,
    device: DeviceId,
    group: GroupId,
    status: LanStatus,
    relay_identity: [u8; 32],
}

/// `--relay-dial-peer`'s parsed value: the peer's relay node id, known out of band (module doc).
type DialPeer = [u8; 32];

/// The background relay transport task; `abort()` on daemon shutdown, same pattern as
/// `LanTransport`.
pub struct RelayTransport {
    task: JoinHandle<()>,
}

impl RelayTransport {
    /// Stops the relay transport. Best-effort: the task may already have exited (bind failed).
    pub fn abort(&self) {
        self.task.abort();
    }
}

/// Starts the relay endpoint as a background task when `relay_url` names one; `None` (or empty)
/// means relay stays off, matching M4/ADR-0024-era behaviour with no fallback carrier at all —
/// `LanStatus::set_relay_configured("")` records that fact for `Health` without spawning anything.
/// `dial_peer` is `--relay-dial-peer` (module doc): when set, this daemon also actively dials that
/// peer over the relay once bound, rather than only accepting incoming connections.
pub fn start(
    ws: SharedWorkspace,
    relay_url: Option<String>,
    dial_peer: Option<DialPeer>,
) -> Option<RelayTransport> {
    let url = relay_url.unwrap_or_default();
    let ctx = {
        let guard = read(&ws);
        RelayCtx {
            ws: ws.clone(),
            device: guard.device(),
            group: guard.group(),
            status: guard.lan_status().clone(),
            relay_identity: guard.relay_identity(),
        }
    };
    ctx.status.set_relay_configured(&url);
    if url.is_empty() {
        return None;
    }
    Some(RelayTransport {
        task: tokio::spawn(run(ctx, url, dial_peer)),
    })
}

async fn bind(ctx: &RelayCtx, url: String) -> Option<Arc<RelayEndpoint>> {
    let cfg = RelayConfig {
        url,
        max_peers: MAX_RELAY_PEERS,
    };
    match RelayEndpoint::bind_with_secret_key(&cfg, ctx.group, ctx.relay_identity).await {
        Ok(e) => {
            let node_id = crate::pairing_wire::hex_encode(&e.node_id_bytes());
            ctx.status
                .set_relay_last_outcome(format!("bound as {node_id}; awaiting connections"));
            let endpoint = Arc::new(e);
            read(&ctx.ws).relay_state().set(Arc::clone(&endpoint));
            Some(endpoint)
        }
        Err(e) => {
            tracing::warn!(error = %e, "relay_bind_failed_running_without_relay_fallback");
            ctx.status
                .set_relay_last_outcome(format!("bind failed: {e}"));
            None
        }
    }
}

fn on_dial_connected(
    ctx: &RelayCtx,
    link: txtodo_sync::IrohLink,
    permit: tokio::sync::OwnedSemaphorePermit,
) {
    ctx.status.set_relay_last_outcome("dialed known peer");
    spawn_driver(ctx.ws.clone(), ctx.device, ctx.group, link, permit);
}

fn log_dial_failed(e: &HolepunchError) {
    tracing::debug!(error = %e, "relay_dial_known_peer_failed");
}

/// `endpoint.connect`, bounded by `lan::CONNECT_TIMEOUT` — `None` on either a real connect failure
/// or a timeout, already logged; split out of [`dial_once`] purely to keep that function's own
/// cognitive complexity under this workspace's budget (clippy.toml).
async fn connect_bounded(
    endpoint: &RelayEndpoint,
    peer: DialPeer,
    group: GroupId,
) -> Option<txtodo_sync::IrohLink> {
    let dial = endpoint.connect(peer, group);
    match tokio::time::timeout(crate::lan::CONNECT_TIMEOUT, dial).await {
        Ok(Ok(link)) => Some(link),
        Ok(Err(e)) => {
            log_dial_failed(&e);
            None
        }
        Err(_) => {
            tracing::debug!("relay_dial_known_peer_timed_out");
            None
        }
    }
}

/// One dial attempt at `peer`; spawns a driver on success. `Ok` permit already reserved by the
/// caller ([`dial_known_peer`]), which owns the retry loop.
async fn dial_once(
    ctx: &RelayCtx,
    endpoint: &RelayEndpoint,
    peer: DialPeer,
    permit: tokio::sync::OwnedSemaphorePermit,
) {
    if let Some(link) = connect_bounded(endpoint, peer, ctx.group).await {
        on_dial_connected(ctx, link, permit);
    }
}

/// `--relay-dial-peer`'s active half (module doc): once `endpoint` is registered with its relay
/// (`RelayEndpoint::online`), repeatedly tries to connect to `peer` and drives a sync session on
/// every success, same as an accepted connection. Keeps retrying on [`DIAL_KNOWN_PEER_INTERVAL`]
/// forever (bounded only by the caller aborting this task on daemon shutdown) rather than a fixed
/// attempt cap: like `lan.rs`'s own resync, a later local edit still needs a fresh dial to reach
/// the peer, and a peer that comes online after this daemon started must still eventually be
/// reached.
async fn dial_known_peer(
    ctx: RelayCtx,
    endpoint: Arc<RelayEndpoint>,
    peer: DialPeer,
    sessions: Arc<Semaphore>,
) {
    endpoint.online().await;
    let mut interval = tokio::time::interval(DIAL_KNOWN_PEER_INTERVAL);
    loop {
        interval.tick().await;
        if let Ok(permit) = Arc::clone(&sessions).try_acquire_owned() {
            dial_once(&ctx, &endpoint, peer, permit).await;
        }
    }
}

/// Bounded the same way `lan.rs::accept_one` is: a relay endpoint flooded with connections must
/// not spawn unbounded tasks either. Dispatches by ALPN exactly like `lan.rs::accept_one` (plan M8
/// `sync-pairing-relay` — pairing-over-relay did not exist when this module's doc above was
/// written; it does now): `PAIRING_ALPN` routes to the pairing handler, everything else to a sync
/// session.
fn on_accepted(link: txtodo_sync::IrohLink, sessions: &Arc<Semaphore>, ctx: &RelayCtx) {
    ctx.status.set_relay_last_outcome("accepted a connection");
    let Ok(permit) = Arc::clone(sessions).try_acquire_owned() else {
        tracing::warn!("relay_session_cap_reached_dropping_incoming");
        return;
    };
    if link.alpn() == PAIRING_ALPN {
        spawn_pairing_driver(ctx.ws.clone(), link, permit);
    } else {
        spawn_driver(ctx.ws.clone(), ctx.device, ctx.group, link, permit);
    }
}

/// One accepted relay pairing connection (this device as initiator) — the relay twin of
/// `lan.rs`'s own private `spawn_pairing_driver`, calling `handle_incoming_over` with `"relay"`
/// instead of `lan.rs`'s `"lan"` so `txtodo doctor` can tell which carrier a completed pairing
/// actually used. Same "blocking thread, one permit" shape as [`spawn_driver`]: `Link::send`/
/// `recv` block (`lan_link.rs`'s own doc).
fn spawn_pairing_driver(
    ws: SharedWorkspace,
    link: txtodo_sync::IrohLink,
    permit: tokio::sync::OwnedSemaphorePermit,
) {
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        let mut link = link;
        crate::pairing_lan::handle_incoming_over(&ws, &mut link, "relay");
    });
}

fn on_no_incoming(ctx: &RelayCtx) {
    tracing::warn!("relay_endpoint_closed");
    ctx.status.set_relay_last_outcome("endpoint closed");
}

fn on_accept_err(e: &HolepunchError, ctx: &RelayCtx) {
    tracing::debug!(error = %e, "relay_accept_failed");
    ctx.status
        .set_relay_last_outcome(format!("accept failed: {e}"));
}

/// One accept iteration; `false` means the endpoint is done and `run`'s loop should stop.
async fn accept_once(endpoint: &RelayEndpoint, sessions: &Arc<Semaphore>, ctx: &RelayCtx) -> bool {
    match endpoint.accept().await {
        Ok(link) => {
            on_accepted(link, sessions, ctx);
            true
        }
        Err(HolepunchError::NoIncoming) => {
            on_no_incoming(ctx);
            false
        }
        Err(e) => {
            on_accept_err(&e, ctx);
            true
        }
    }
}

async fn run(ctx: RelayCtx, url: String, dial_peer: Option<DialPeer>) {
    let Some(endpoint) = bind(&ctx, url).await else {
        return;
    };
    let sessions = Arc::new(Semaphore::new(MAX_CONCURRENT_LAN_SESSIONS));
    if let Some(peer) = dial_peer {
        tokio::spawn(dial_known_peer(
            ctx.clone(),
            Arc::clone(&endpoint),
            peer,
            Arc::clone(&sessions),
        ));
    }
    while accept_once(&endpoint, &sessions, &ctx).await {}
}
