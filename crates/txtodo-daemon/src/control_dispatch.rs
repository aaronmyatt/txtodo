//! Dispatches one connection accepted on the device's one shared relay endpoint
//! (`control_channel.rs`'s accept loop, task `daemon-shared-sync-link` stage 3) by which ALPN it
//! negotiated — split out of `control_channel.rs` for that file's line budget, the same pattern
//! this crate already uses for `control_session.rs`. Before this stage, that accept loop silently
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
//! - the regular sync `ALPN` → this is the actual routing problem stage 1's `peek_workspace`
//!   exists for: read the connection's first frame, peek its clear-header `workspace_id`, look it
//!   up in `WorkspaceRoutes`, and hand the connection to the *existing, unchanged*
//!   `lan_session::drive_session` for that workspace — via [`ReplayFirstFrame`], a `Link` adapter
//!   that replays the one already-read frame on its first `recv()` and delegates to the real link
//!   afterward, so `Session`/`Message` never know a peek happened above them. An unknown/not-yet-
//!   open workspace is logged and dropped, same "logged, never fatal" precedent as every other
//!   accept-loop failure in this crate.
//!
//! Every branch that touches `Link::send`/`recv` runs entirely inside its own `spawn_blocking`
//! closure (`txtodo-sync/CLAUDE.md`'s own invariant: those calls block a dedicated driver thread,
//! and calling either from a plain tokio task would starve the runtime) — the ALPN check and the
//! `WorkspaceRoutes::any` pairing lookup are the only things this module ever does on the caller's
//! own async task, since neither touches the wire.

use std::sync::{Arc, Mutex};

use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use txtodo_sync::{ALPN, CONTROL_ALPN, Frame, IrohLink, Link, LinkError, PAIRING_ALPN};

use crate::device_identity::DeviceIdentity;
use crate::device_relay::{DeviceRelay, WorkspaceRoute, WorkspaceRoutes};
use crate::lan_session::drive_session;
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

/// A `Link` that replays one already-read `Frame` on its first `recv()`, then delegates to the
/// wrapped link for everything after — lets a caller peek a connection's first frame to route it,
/// then hand the connection to code (`drive_session`) that still expects to read that same first
/// frame itself, without either side knowing routing happened.
struct ReplayFirstFrame<'a> {
    inner: &'a mut dyn Link,
    buffered: Option<Frame>,
}

impl<'a> ReplayFirstFrame<'a> {
    fn new(inner: &'a mut dyn Link, first: Frame) -> ReplayFirstFrame<'a> {
        ReplayFirstFrame {
            inner,
            buffered: Some(first),
        }
    }
}

impl Link for ReplayFirstFrame<'_> {
    fn send(&mut self, frame: Frame) -> Result<(), LinkError> {
        self.inner.send(frame)
    }

    fn recv(&mut self) -> Result<Frame, LinkError> {
        match self.buffered.take() {
            Some(frame) => Ok(frame),
            None => self.inner.recv(),
        }
    }
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
        return dispatch_sync(link, Arc::clone(&ctx.device_relay), permit);
    }
    tracing::debug!("control_dispatch_unknown_alpn_dropping_incoming");
}

fn dispatch_control(link: IrohLink, ctx: &DispatchCtx, permit: OwnedSemaphorePermit) {
    crate::control_channel::spawn_session(
        link,
        Arc::clone(&ctx.identity),
        Arc::clone(&ctx.registry),
        permit,
    );
}

fn dispatch_pairing(link: IrohLink, device_relay: &DeviceRelay, permit: OwnedSemaphorePermit) {
    let Some(route) = device_relay.routes().any() else {
        tracing::debug!("control_dispatch_pairing_no_open_workspace_dropping_incoming");
        return;
    };
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        let mut link = link;
        crate::pairing_lan::handle_incoming_over(&route.ws, &mut link, "relay");
    });
}

fn dispatch_sync(link: IrohLink, device_relay: Arc<DeviceRelay>, permit: OwnedSemaphorePermit) {
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        let mut link = link;
        drive_routed_sync_connection(&mut link, device_relay.routes());
    });
}

/// The blocking body of [`dispatch_sync`]'s spawned task: reads the connection's first frame,
/// peeks its workspace id, routes and drives, each step logged and dropped on failure rather than
/// propagated (there is no caller left to hand an error back to once this is running on its own
/// blocking thread). Takes `&mut dyn Link` and a bare [`WorkspaceRoutes`] rather than the concrete
/// `IrohLink`/`DeviceRelay` `dispatch_sync` itself must use (only `IrohLink::alpn()`, checked one
/// layer up, needs the real endpoint type; this function never touches it at all) — this is what
/// lets `control_dispatch_tests.rs` drive it end to end over a real `ChannelLink` with a bare
/// routing table, no real network bind required, the same "one side real, one side scripted"
/// harness `lan_session_tests.rs` established.
pub(crate) fn drive_routed_sync_connection(link: &mut dyn Link, routes: &WorkspaceRoutes) {
    let Some((route, first_frame)) = route_first_frame(link, routes) else {
        return;
    };
    let mut replay = ReplayFirstFrame::new(link, first_frame);
    drive_session(&mut replay, route.ws, route.device, route.group);
}

/// Reads `link`'s first frame and resolves the open workspace it's for — `None` on any failure
/// along the way (a real recv error, too short to peek, or an unknown/not-yet-open workspace),
/// each logged at its own step (by [`recv_first_frame`]/[`peek_workspace_logged`]/
/// [`route_logged`]) so the reason a connection was dropped stays visible. Split into one tiny
/// function per step purely to keep each one's own cognitive complexity under this workspace's
/// budget (`clippy.toml`).
fn route_first_frame(
    link: &mut dyn Link,
    routes: &WorkspaceRoutes,
) -> Option<(WorkspaceRoute, Frame)> {
    let first_frame = recv_first_frame(link)?;
    let workspace_id = peek_workspace_logged(&first_frame)?;
    let route = route_logged(routes, workspace_id)?;
    Some((route, first_frame))
}

fn recv_first_frame(link: &mut dyn Link) -> Option<Frame> {
    link.recv()
        .inspect_err(|e| tracing::debug!(error = %e, "control_dispatch_sync_recv_failed"))
        .ok()
}

fn peek_workspace_logged(frame: &Frame) -> Option<txtodo_store::WorkspaceId> {
    let id = txtodo_sync::peek_workspace(&frame.body);
    if id.is_none() {
        tracing::debug!("control_dispatch_sync_first_frame_too_short_dropping");
    }
    id
}

fn route_logged(
    routes: &WorkspaceRoutes,
    workspace_id: txtodo_store::WorkspaceId,
) -> Option<WorkspaceRoute> {
    let route = routes.route(workspace_id);
    if route.is_none() {
        tracing::debug!(
            %workspace_id,
            "control_dispatch_sync_unknown_workspace_dropping_incoming"
        );
    }
    route
}
