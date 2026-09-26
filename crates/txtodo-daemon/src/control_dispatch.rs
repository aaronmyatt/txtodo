//! Dispatches one connection accepted on the device's one shared relay endpoint
//! (`control_channel.rs`'s accept loop, task `daemon-shared-sync-link` stage 3) by which ALPN it
//! negotiated — split out of `control_channel.rs` for that file's line budget, the same pattern
//! this crate already uses for `control_session.rs`. Before stage 3, that accept loop silently
//! dropped every connection that was not `CONTROL_ALPN`; now all three of this device's protocols
//! share the one bound endpoint (`DeviceRelay::bind`, stage 2) and are told apart here:
//!
//! - `CONTROL_ALPN` → the existing, unchanged `control_session::drive_control_session`
//!   (`control_channel::spawn_session`).
//! - `PAIRING_ALPN` → `pairing_lan::handle_incoming_over`, the same handler `relay.rs`'s own
//!   accept loop already calls for a per-workspace endpoint — it only ever reaches the shared
//!   `DeviceIdentity` through its `&SharedWorkspace` argument (ADR 0021), so *any* open
//!   workspace's route will do (`WorkspaceRoutes::any`); no workspace open at all means no route
//!   to hand it, logged and dropped.
//! - the regular sync `ALPN` → task `daemon-workspace-session-multiplex` stage 2: every open,
//!   routed workspace this device has (`device_relay.routes()`) is handed to
//!   `lan_session_dispatch::drive_shared_session` as one shared connection, interleaving
//!   `Greet`/`Want`/`Ops`/`Ack` across all of them rather than peeking the connection's first
//!   frame to route it to exactly one. Before this stage, this branch did exactly that peek-and-
//!   route-to-one-workspace dance (`ReplayFirstFrame`, now gone) — see
//!   `tasks/daemon-workspace-session-multiplex/notes.md` for why that is no longer needed: the
//!   accept side already knows every workspace it has open without reading a single byte off the
//!   wire, and the *per-message* demuxing that peek was really standing in for now happens once
//!   per frame, inside the shared driver's own read loop.
//!
//! Every branch that touches `Link::send`/`recv` runs entirely inside its own `spawn_blocking`
//! closure (`txtodo-sync/CLAUDE.md`'s own invariant: those calls block a dedicated driver thread,
//! and calling either from a plain tokio task would starve the runtime) — the ALPN check and the
//! `WorkspaceRoutes::any` pairing lookup are the only things this module ever does on the caller's
//! own async task, since neither touches the wire.

use std::sync::{Arc, Mutex};

use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use txtodo_sync::{ALPN, CONTROL_ALPN, IrohLink, PAIRING_ALPN};

use crate::device_identity::DeviceIdentity;
use crate::device_relay::DeviceRelay;
use crate::lan_session_dispatch::drive_shared_session;
use crate::live_peers::Carrier;
use crate::workspace_registry::WorkspaceRegistry;

/// The device-level state every dispatch branch might need, bundled so the functions below stay
/// under the argument-count cap — the same reason `lan.rs`'s `LanCtx`/`relay.rs`'s `RelayCtx`
/// exist.
#[derive(Clone)]
pub(crate) struct DispatchCtx {
    pub(crate) identity: Arc<DeviceIdentity>,
    pub(crate) registry: Arc<Mutex<WorkspaceRegistry>>,
    pub(crate) device_relay: Arc<DeviceRelay>,
}

/// Acquires a permit from `sem` (logging and dropping the connection at the cap, the same message
/// `control_channel.rs` already logged for `CONTROL_ALPN` before this stage) and dispatches `link`
/// by its negotiated ALPN.
pub(crate) fn accept_one(link: IrohLink, ctx: &DispatchCtx, sem: &Arc<Semaphore>, cap: usize) {
    let Ok(permit) = Arc::clone(sem).try_acquire_owned() else {
        tracing::warn!(cap, "control_channel_session_cap_reached_dropping_incoming");
        return;
    };
    dispatch_accepted(link, ctx, permit);
}

fn dispatch_accepted(link: IrohLink, ctx: &DispatchCtx, permit: OwnedSemaphorePermit) {
    let alpn = link.alpn();
    if alpn == CONTROL_ALPN {
        return dispatch_control(link, ctx, permit);
    }
    if alpn == PAIRING_ALPN {
        return dispatch_pairing(link, &ctx.device_relay, permit);
    }
    if alpn == ALPN {
        return dispatch_sync(link, ctx, permit);
    }
    tracing::debug!("control_dispatch_unknown_alpn_dropping_incoming");
}

fn dispatch_control(link: IrohLink, ctx: &DispatchCtx, permit: OwnedSemaphorePermit) {
    crate::control_channel::spawn_session(
        link,
        Arc::clone(&ctx.identity),
        Arc::clone(&ctx.registry),
        permit,
        None,
    );
}

fn dispatch_pairing(link: IrohLink, device_relay: &DeviceRelay, permit: OwnedSemaphorePermit) {
    let Some(route) = device_relay.routes().any() else {
        tracing::warn!("control_dispatch_pairing_no_open_workspace_dropping_incoming");
        return;
    };
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        let mut link = link;
        crate::pairing_lan::handle_incoming_over(&route.ws, &mut link, "relay");
    });
}

/// `device`/`group` come from `ctx.identity` (ADR 0021: one device id and one sync group for the
/// whole device-set, shared by every workspace) rather than from anything peeked off the wire —
/// stage 2's whole point is that the accept side already knows this without reading a byte.
fn dispatch_sync(link: IrohLink, ctx: &DispatchCtx, permit: OwnedSemaphorePermit) {
    let device_relay = Arc::clone(&ctx.device_relay);
    let device = ctx.identity.device();
    let group = ctx.identity.group();
    let keys = ctx.identity.peer_keys().clone();
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        let mut link = link;
        let end = drive_shared_session(
            &mut link,
            device_relay.routes(),
            device,
            group,
            Carrier::Relay,
        );
        // An incoming session: its peer is known only once its `Hello` opens.
        keys.book_session(None, end);
    });
}
