//! Wires this device's one shared relay endpoint (task `daemon-shared-sync-link` stage 5,
//! `DeviceRelay::bind` in `main.rs::run`) into a running workspace: registers the endpoint against
//! this workspace's own [`crate::relay_state::RelayState`] (so `lan.rs`'s relay-fallback dial and
//! `pairing_grpc.rs`'s offer rendezvous can reuse it) and, when `--relay-dial-peer` names one,
//! spawns the outbound dial/redial loop that reaches it. ADR 0026: relay is additive, LAN stays
//! primary.
//!
//! **No longer this module's job**: binding the endpoint at all, and accepting/dispatching
//! incoming relay connections. Before task `daemon-shared-sync-link`, every open workspace bound
//! its *own* `RelayEndpoint` under this device's one persisted relay identity and ran its own
//! accept loop — which is exactly the bug that task exists to fix (two or more `iroh::Endpoint`s
//! sharing one identity make a real relay server refuse the second connection outright). The one
//! shared endpoint is bound once, in `main.rs`, before any workspace opens; accepting and
//! dispatching every connection it receives (by ALPN, then — for a sync connection — by peeked
//! `workspace_id`) is `control_dispatch.rs`'s job now. This module's remaining job, the *dialing*
//! half of the LAN→relay fallback (this device reaching a peer over relay when LAN can't), is
//! unchanged: `lan.rs::dial_and_spawn`, via `relay_fallback.rs` and `RelayState`, for a peer LAN
//! *discovered* but could not reach.
//!
//! **`--relay-dial-peer` (plan M8 `relay-converge-test`): the rendezvous gap that pass left open.**
//! `relay_fallback_dial`'s own doc names a real limitation — it dials a peer's *LAN* node id over
//! relay, reachable only once both carriers share one identity, not built yet. Investigating it for
//! that task surfaced a second, deeper gap: `lan.rs::dial_and_spawn` (and therefore
//! `relay_fallback_dial`) only ever runs for a peer `handle_sighting` already learned about via
//! **mDNS**, which by construction never crosses a real network boundary — two daemons that were
//! never on the same LAN never populate each other's `PeerTable` at all, so the relay fallback path
//! is simply never reached for them, identity-sharing aside. Real pairing-over-relay (a rendezvous
//! protocol that works with no shared LAN) is `sync-pairing-relay`'s own not-yet-built task per ADR
//! 0026's follow-up list — out of scope here to build in full. `--relay-dial-peer <hex node id>`
//! is that task's minimal, honestly-scoped substitute: a caller who already knows a peer's *relay*
//! node id (e.g. read off that peer's own `Health.relay_last_outcome`, or a test harness that
//! seeded it) can hand it to this daemon at startup, and [`dial_known_peer`] below drives the
//! connect/sync loop directly over the shared `RelayEndpoint` — no LAN discovery, no shared LAN,
//! ever required. This sidesteps the identity-sharing gap too: it dials the peer's *actual* relay
//! identity, never conflates it with a LAN one.
//!
//! **Known scope limit, deliberate**: unlike `lan.rs::rebuild_on_group_change`, this module never
//! rebinds anything when the workspace's sync group changes (e.g. mid-run pairing) — nor could it
//! now, since binding is a device-level concern this module no longer performs at all. Pairing-
//! over-relay is a separate, not-yet-built task (`sync-pairing-relay`, scoped by ADR 0026's own
//! follow-up list) — out of scope here.

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::Semaphore;
use tokio::task::JoinHandle;
use txtodo_model::DeviceId;
use txtodo_sync::{GroupId, HolepunchError, RelayEndpoint};

use crate::device_relay::DeviceRelay;
use crate::lan::MAX_CONCURRENT_LAN_SESSIONS;
use crate::lan_session::read;
use crate::lan_session_dispatch::drive_shared_session;
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
}

/// `--relay-dial-peer`'s parsed value: the peer's relay node id, known out of band (module doc).
type DialPeer = [u8; 32];

/// The background relay dial task; `abort()` on daemon shutdown, same pattern as `LanTransport`.
/// `None` from [`start`] (no dial peer configured) means there is nothing to abort at all —
/// registering the shared endpoint against this workspace happens synchronously, not as a task.
/// `task` is itself `Option`al (not just the outer `RelayTransport`) since stage 2: a workspace
/// that registered but lost the device-level dial claim (`DeviceRelay::claim_dial`) still gets a
/// `RelayTransport` back (so callers need no `None`-means-"registration failed" special case), it
/// just owns nothing to abort — see [`start`]'s own doc for the known ownership limitation this
/// implies.
pub struct RelayTransport {
    task: Option<JoinHandle<()>>,
}

impl RelayTransport {
    /// Stops the dial loop, if this workspace is the one that owns it. Best-effort: the task may
    /// already have exited.
    pub fn abort(&self) {
        if let Some(task) = &self.task {
            task.abort();
        }
    }
}

/// Registers this workspace against the device's one shared relay endpoint (`None` when `--relay`
/// was never configured for this device, or its bind failed — either way there is nothing to
/// register or dial through). `relay_url` is reporting-only (`Health.relay_url`): the endpoint
/// itself was already bound once in `main.rs`, not from this call, so this function's only job
/// with it is recording what was configured, matching `LanStatus::set_relay_configured`'s existing
/// contract. `dial_peer` (`--relay-dial-peer`) is this workspace's own active half; registration
/// itself needs no background task, so `None` dial-peer with a present endpoint returns `None`
/// (nothing to abort) after registering synchronously.
///
/// Task `daemon-workspace-session-multiplex` stage 2: `open_workspace_full` calls this once per
/// *workspace*, but a dial task must exist at most once per *device* — `device_relay.claim_dial`
/// is what lets a second (or third...) open workspace with the same `--relay-dial-peer` register
/// without spawning a second, redundant dial loop that would just race the first one to connect.
/// Only the workspace whose call actually wins the claim gets a `RelayTransport` with something to
/// abort; every other one still registers (so its own `Health`/`RelayState` are correct) but gets
/// `RelayTransport { task: None }` — a known, flagged limitation: if *that* first workspace closes
/// while others with the same dial peer remain open, the shared dial task stops with it, since
/// there is no single, longer-lived owner below the device level to hand it to instead. Building
/// that owner is real further work this stage does not attempt; the daemon process itself tearing
/// down (which drops every task together) is unaffected.
pub fn start(
    ws: SharedWorkspace,
    relay_url: Option<String>,
    device_relay: Option<Arc<DeviceRelay>>,
    dial_peer: Option<DialPeer>,
) -> Option<RelayTransport> {
    let ctx = build_ctx(&ws);
    ctx.status
        .set_relay_configured(relay_url.as_deref().unwrap_or_default());
    let device_relay = device_relay?;
    register(&ctx, &device_relay.endpoint());
    let peer = dial_peer?;
    if !device_relay.claim_dial(peer) {
        return Some(RelayTransport { task: None });
    }
    Some(RelayTransport {
        task: Some(tokio::spawn(dial_known_peer(
            ctx,
            device_relay,
            peer,
            Arc::new(Semaphore::new(MAX_CONCURRENT_LAN_SESSIONS)),
        ))),
    })
}

fn build_ctx(ws: &SharedWorkspace) -> RelayCtx {
    let guard = read(ws);
    RelayCtx {
        ws: ws.clone(),
        device: guard.device(),
        group: guard.group(),
        status: guard.lan_status().clone(),
    }
}

/// Records the shared endpoint's identity for `Health`/`txtodo doctor` and makes it reachable
/// from `ws.relay_state()` — same outcome string `bind()` used to log here before this task, since
/// `support::relay::parse_relay_node_id` (this crate's own test harness) still parses it.
fn register(ctx: &RelayCtx, endpoint: &Arc<RelayEndpoint>) {
    let node_id = crate::pairing_wire::hex_encode(&endpoint.node_id_bytes());
    ctx.status
        .set_relay_last_outcome(format!("bound as {node_id}; awaiting connections"));
    read(&ctx.ws).relay_state().set(Arc::clone(endpoint));
}

/// Stage 2: drives the new connection over *every* workspace `device_relay.routes()` currently
/// names, not just `ctx.ws` alone — the whole point of deduplicating the dial task
/// (`DeviceRelay::claim_dial`) is that one connection now serves every workspace sharing this
/// `--relay-dial-peer`, interleaved, instead of one connection per workspace.
fn on_dial_connected(
    ctx: &RelayCtx,
    device_relay: &Arc<DeviceRelay>,
    link: txtodo_sync::IrohLink,
    permit: tokio::sync::OwnedSemaphorePermit,
) {
    ctx.status.set_relay_last_outcome("dialed known peer");
    let device_relay = Arc::clone(device_relay);
    let (device, group) = (ctx.device, ctx.group);
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        let mut link = link;
        drive_shared_session(&mut link, device_relay.routes(), device, group);
    });
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
    device_relay: &Arc<DeviceRelay>,
    peer: DialPeer,
    permit: tokio::sync::OwnedSemaphorePermit,
) {
    if let Some(link) = connect_bounded(&device_relay.endpoint(), peer, ctx.group).await {
        on_dial_connected(ctx, device_relay, link, permit);
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
    device_relay: Arc<DeviceRelay>,
    peer: DialPeer,
    sessions: Arc<Semaphore>,
) {
    device_relay.endpoint().online().await;
    let mut interval = tokio::time::interval(DIAL_KNOWN_PEER_INTERVAL);
    loop {
        interval.tick().await;
        if let Ok(permit) = Arc::clone(&sessions).try_acquire_owned() {
            dial_once(&ctx, &device_relay, peer, permit).await;
        }
    }
}
