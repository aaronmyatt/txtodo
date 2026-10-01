//! The device's one LAN transport (task `sync-live-push`, 2026-09-24): which open workspaces an
//! incoming or dialed LAN connection carries, and starting the one LAN task. Mirrors
//! `DeviceRelay`: every workspace registers its route here when it opens
//! (`workspace_catalog_open.rs`) and unregisters on close, and `lan.rs` hands this table to
//! `drive_shared_session` for every connection, so one LAN link multiplexes them all.

use std::sync::{Arc, Mutex};

use tokio::sync::Semaphore;
use txtodo_sync::{CONTROL_ALPN, IrohLink, LanEndpoint, PAIRING_ALPN};

use crate::clock::Clock;
use crate::device_identity::DeviceIdentity;
use crate::device_relay::{DeviceRelay, WorkspaceRoutes};
use crate::lan::{
    CONNECT_TIMEOUT, LanCtx, LanTransport, MAX_CONCURRENT_LAN_SESSIONS, spawn_driver,
};
use crate::lan_peers::{KnownPeers, peers_to_resync};
use crate::live_peers::Carrier;
use crate::server::SharedWorkspace;
use crate::workspace_registry::WorkspaceRegistry;

/// The LAN side's routing table. Its own type (not a bare `WorkspaceRoutes`) so it reads like its
/// relay and file-carrier siblings at every call site.
#[derive(Default)]
pub struct DeviceLan {
    routes: WorkspaceRoutes,
}

impl DeviceLan {
    /// Every workspace this device has open for LAN sync.
    pub fn routes(&self) -> &WorkspaceRoutes {
        &self.routes
    }
}

/// Opens the registry handle the LAN control sessions read offers from — its own handle, like the
/// relay control channel's (`control_channel.rs::open_registry`). `None` (logged) keeps LAN sync
/// running with no offers over LAN.
pub fn open_registry(path: &std::path::Path) -> Option<Arc<Mutex<WorkspaceRegistry>>> {
    match WorkspaceRegistry::open(path) {
        Ok(r) => Some(Arc::new(Mutex::new(r))),
        Err(e) => {
            tracing::warn!(error = %e, "lan_control_registry_open_failed");
            None
        }
    }
}

/// What [`start`] runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LanMode {
    /// Nothing: a keystore that cannot keep keys turns sync off.
    Off,
    /// `--no-lan`: no LAN endpoint or discovery, only the relay-only auto-dial tick that the LAN
    /// task used to carry (`relay_autodial::run_without_lan`). Before that, `--no-lan` turned the
    /// auto-dial off too (p2p lab finding). Without a bound relay this is [`LanMode::Off`].
    RelayOnly,
    /// The LAN task, whose resync tick also runs the relay-only auto-dial.
    Full,
}

impl LanMode {
    /// The mode for `--no-lan` and whether this device's keystore allows sync at all.
    pub fn from_flags(no_lan: bool, sync_allowed: bool) -> LanMode {
        match (sync_allowed, no_lan) {
            (false, _) => LanMode::Off,
            (true, true) => LanMode::RelayOnly,
            (true, false) => LanMode::Full,
        }
    }
}

/// Starts the one LAN task, or under [`LanMode::RelayOnly`] just its relay-only auto-dial.
/// Returns the table workspaces register on (relay-only sessions drive over it too), and the task
/// handle to keep alive. `registry` feeds the control sessions that carry workspace offers over
/// LAN.
pub fn start(
    mode: LanMode,
    identity: Arc<DeviceIdentity>,
    device_relay: Option<Arc<DeviceRelay>>,
    clock: Arc<dyn Clock>,
    registry: Option<Arc<Mutex<WorkspaceRegistry>>>,
) -> Option<(Arc<DeviceLan>, LanTransport)> {
    if mode == LanMode::Off || (mode == LanMode::RelayOnly && device_relay.is_none()) {
        return None;
    }
    let lan = Arc::new(DeviceLan::default());
    let ctx = LanCtx {
        device: identity.device(),
        group: identity.group(),
        identity,
        lan: Arc::clone(&lan),
        device_relay,
        clock,
        registry,
    };
    let task = match mode {
        LanMode::RelayOnly => crate::relay_autodial::run_without_lan(ctx),
        _ => crate::lan::start(ctx),
    };
    Some((lan, task))
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
pub(crate) fn accept_one(
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
    if link.alpn() == CONTROL_ALPN {
        accept_control(link, ctx, permit);
    } else if link.alpn() == PAIRING_ALPN {
        accept_pairing(link, ctx, permit);
    } else {
        // An incoming session: its peer is known only once its `Hello` opens.
        let keys = ctx.identity.peer_keys().clone();
        spawn_driver(ctx.clone(), (link, Carrier::Lan), permit, move |end| {
            keys.book_session(None, end);
        });
    }
}

/// A peer's workspace-offer session over LAN, driven exactly like a relay one.
fn accept_control(link: IrohLink, ctx: &LanCtx, permit: tokio::sync::OwnedSemaphorePermit) {
    let Some(registry) = &ctx.registry else {
        return log_dropped("lan_control_no_registry_dropping_incoming");
    };
    crate::control_channel::spawn_session(
        link,
        Arc::clone(&ctx.identity),
        Arc::clone(registry),
        permit,
        None,
    );
}

/// Pairing reaches only the shared `DeviceIdentity` through a workspace (ADR 0021), so any open
/// one will do; none open means nothing to pair into yet.
fn accept_pairing(link: IrohLink, ctx: &LanCtx, permit: tokio::sync::OwnedSemaphorePermit) {
    match ctx.lan.routes().any() {
        Some(route) => spawn_pairing_driver(route.ws, link, permit),
        None => log_dropped("lan_pairing_no_open_workspace_dropping_incoming"),
    }
}

fn log_dropped(event: &'static str) {
    tracing::warn!(event, "lan_incoming_dropped");
}

/// Workspace offers over LAN (task `default-workspace`'s LAN pairing test): on every resync tick,
/// one short control session with each LAN peer this device dials (the lower id, like sync), the
/// same exchange the relay control channel runs. The session ends on its own once both sides went
/// quiet (`IrohLink`'s idle close).
pub(crate) fn dial_control(
    known_peers: &KnownPeers,
    ctx: &LanCtx,
    endpoint: &Arc<LanEndpoint>,
    sessions: &Arc<Semaphore>,
) {
    let Some(registry) = ctx.registry.clone() else {
        return;
    };
    for peer in peers_to_resync(known_peers, &ctx.identity) {
        let Ok(permit) = Arc::clone(sessions).try_acquire_owned() else {
            return;
        };
        let (endpoint, identity, registry) = (
            Arc::clone(endpoint),
            Arc::clone(&ctx.identity),
            Arc::clone(&registry),
        );
        tokio::spawn(async move {
            // Bounded: a black-holed peer must not hold a permit forever.
            let dial = endpoint.connect_control(peer.node, &peer.addresses);
            match tokio::time::timeout(CONNECT_TIMEOUT, dial).await {
                Ok(Ok(link)) => {
                    let dialed = Some(peer.device);
                    crate::control_channel::spawn_session(link, identity, registry, permit, dialed);
                }
                Ok(Err(e)) => log_control_dial_failed(peer.device, &e.to_string()),
                Err(_) => log_control_dial_failed(peer.device, "timed out"),
            }
        });
    }
}

fn log_control_dial_failed(peer: txtodo_model::DeviceId, error: &str) {
    tracing::debug!(%peer, error, "lan_control_dial_failed");
}

#[cfg(test)]
mod tests {
    use super::LanMode;

    /// `--no-lan` keeps the relay-only auto-dial (p2p lab finding); only a keystore that cannot
    /// keep keys turns everything off.
    #[test]
    fn no_lan_keeps_the_relay_only_auto_dial() {
        assert_eq!(LanMode::from_flags(true, true), LanMode::RelayOnly);
        assert_eq!(LanMode::from_flags(false, true), LanMode::Full);
        assert_eq!(LanMode::from_flags(true, false), LanMode::Off);
        assert_eq!(LanMode::from_flags(false, false), LanMode::Off);
    }
}
