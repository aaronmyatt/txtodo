//! The device's one LAN transport (task `sync-live-push`, 2026-09-24): which open workspaces an
//! incoming or dialed LAN connection carries, and starting the one LAN task. Mirrors
//! `DeviceRelay`: every workspace registers its route here when it opens
//! (`workspace_catalog_open.rs`) and unregisters on close, and `lan.rs` hands this table to
//! `drive_shared_session` for every connection, so one LAN link multiplexes them all.

use std::sync::Arc;

use tokio::sync::Semaphore;
use txtodo_sync::{IrohLink, PAIRING_ALPN};

use crate::clock::Clock;
use crate::device_identity::DeviceIdentity;
use crate::device_relay::{DeviceRelay, WorkspaceRoutes};
use crate::lan::{LanCtx, LanTransport, MAX_CONCURRENT_LAN_SESSIONS, spawn_driver};
use crate::server::SharedWorkspace;

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

/// Starts the one LAN task unless `enabled` is false (`--no-lan`, or a keystore that cannot keep
/// keys). Returns the table workspaces register on, and the task handle to keep alive.
pub fn start(
    enabled: bool,
    identity: Arc<DeviceIdentity>,
    device_relay: Option<Arc<DeviceRelay>>,
    clock: Arc<dyn Clock>,
) -> Option<(Arc<DeviceLan>, LanTransport)> {
    if !enabled {
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
    };
    Some((lan, crate::lan::start(ctx)))
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
    if link.alpn() == PAIRING_ALPN {
        // Pairing reaches only the shared `DeviceIdentity` through a workspace (ADR 0021), so any
        // open one will do; none open means nothing to pair into yet.
        match ctx.lan.routes().any() {
            Some(route) => spawn_pairing_driver(route.ws, link, permit),
            None => tracing::warn!("lan_pairing_no_open_workspace_dropping_incoming"),
        }
    } else {
        spawn_driver(ctx.clone(), link, permit, |_| {});
    }
}
