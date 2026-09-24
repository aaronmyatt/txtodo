//! The actual read/write loop and entry point for task `daemon-workspace-session-multiplex`
//! stage 2 — split out of `lan_session_shared.rs` for its line budget; that file owns the wire
//! primitives (`send_message`/`open_and_decode_logged`) and the per-workspace message
//! handlers (`handle_workspace_message` and everything it calls), both reused here. See that
//! file's module doc for the wire sequence and failure scope this loop implements.

use std::collections::BTreeMap;
use std::sync::PoisonError;

use tokio::runtime::Handle;
use txtodo_model::DeviceId;
use txtodo_store::{StoreError, WorkspaceId};
use txtodo_sync::{
    DeviceSigningKey, GroupId, GroupKey, GroupKeys, Link, Session, derive_group_op_signing_key,
    peek_workspace,
};

use crate::device_relay::{WorkspaceRoute, WorkspaceRoutes};
use crate::lan_session::{fetch_group_key, read, read_heads, single_epoch_keys};
use crate::lan_session_live::{Live, POLL, PushCtx};
use crate::lan_session_shared::{
    LINK_WORKSPACE, MAX_MESSAGES_PER_SESSION, SessionCtx, handle_link_hello,
    handle_workspace_message, open_and_decode_logged, send_message,
};
use crate::live_peers::LivePeers;

/// Everything shared across every workspace this one connection multiplexes: the crypto material
/// (one group key for the whole device-set, ADR 0021) and the routing table naming which
/// `SharedWorkspace` each locally open workspace resolves to.
struct SharedCtx<'a> {
    /// The live route table this session was built from, and its generation then.
    table: &'a WorkspaceRoutes,
    generation: u64,
    rt: Handle,
    group: GroupId,
    key: GroupKey,
    keys: GroupKeys,
    signing_key: DeviceSigningKey,
    routes: BTreeMap<WorkspaceId, WorkspaceRoute>,
    live_peers: LivePeers,
}

/// The per-connection state every frame updates: the protocol `Session` and the push/liveness
/// bookkeeping (task `sync-live-push`), bundled so the dispatch functions stay under `maxParams`.
struct Conn {
    session: Session,
    live: Live,
}

/// Any one routed workspace's own clock, for stamping the link-level `Hello` and re-checking skew
/// on each incoming one — every workspace this daemon opens shares one injected `Clock` in
/// practice (`Workspace::clock`, ADR 0021), so which one answers does not matter.
fn any_route_now_ms(routes: &BTreeMap<WorkspaceId, WorkspaceRoute>) -> u64 {
    routes
        .values()
        .next()
        .map(|route| read(&route.ws).clock().now_ms())
        .unwrap_or(0)
}

fn log_frame_too_short_to_peek() {
    tracing::debug!("lan_session_frame_too_short_to_peek");
}

fn log_unrouted_workspace_skipped(workspace: WorkspaceId) {
    tracing::debug!(%workspace, "lan_session_unrouted_workspace_message_skipped");
}

fn log_touch_last_seen_unknown_device(peer: DeviceId) {
    tracing::debug!(peer = %peer, "lan_session_touch_last_seen_unknown_device");
}

fn log_touch_last_seen_failed(peer: DeviceId, error: &StoreError) {
    tracing::warn!(peer = %peer, error = %error, "lan_session_touch_last_seen_failed");
}

/// Marks `peer` seen right now, fixing a real, pre-existing gap: registration only ever sets
/// `last_seen` once, at pairing time (`devices_grpc.rs::sync_status_impl`'s doc has the full
/// history) — nothing ever advanced it again. This runs right after `peer`'s link-level `Hello`
/// validates (`dispatch_link_frame`, below) — the one place every real sync session (LAN, relay,
/// control channel all converge on `drive_shared_session`) learns the peer's device id, so it is
/// the one place this needs wiring, not three. Any routed workspace's identity store works, same
/// reasoning as [`any_route_now_ms`] — the device-global `devices` table is shared across every
/// workspace this daemon has open (ADR 0021).
fn touch_peer_last_seen(
    routes: &BTreeMap<WorkspaceId, WorkspaceRoute>,
    peer: DeviceId,
    now_ms: u64,
) {
    let Some(route) = routes.values().next() else {
        return;
    };
    let result = read(&route.ws)
        .identity_store()
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .touch_last_seen(peer, now_ms);
    match result {
        Ok(true) => {}
        Ok(false) => log_touch_last_seen_unknown_device(peer),
        Err(e) => log_touch_last_seen_failed(peer, &e),
    }
}

/// The link-level `Hello` branch of [`dispatch_frame`] — split out purely to keep that
/// function's own cognitive complexity under this workspace's budget (`clippy.toml`).
fn dispatch_link_frame(
    frame: &txtodo_sync::Frame,
    shared: &SharedCtx<'_>,
    conn: &mut Conn,
) -> Option<()> {
    let msg = open_and_decode_logged(frame, shared.group, LINK_WORKSPACE, &shared.keys)?;
    let now_ms = any_route_now_ms(&shared.routes);
    if !handle_link_hello(&mut conn.session, &msg, now_ms) {
        return None;
    }
    if let Some(peer) = conn.session.peer() {
        touch_peer_last_seen(&shared.routes, peer, now_ms);
        conn.live.enter(&shared.live_peers, peer);
    }
    Some(())
}

/// A real workspace's own branch of [`dispatch_frame`] — `Some(())` (not `None`) when
/// `workspace` has no local route: an unrouted workspace is skipped, never fatal to the
/// connection (`lan_session_shared.rs`'s module doc, "failure scope").
fn dispatch_workspace_frame(
    link: &mut dyn Link,
    frame: &txtodo_sync::Frame,
    shared: &SharedCtx<'_>,
    conn: &mut Conn,
    workspace: WorkspaceId,
) -> Option<()> {
    let Some(route) = shared.routes.get(&workspace) else {
        log_unrouted_workspace_skipped(workspace);
        return Some(());
    };
    let msg = open_and_decode_logged(frame, shared.group, workspace, &shared.keys)?;
    conn.live.observe(workspace, &msg);
    let ctx = SessionCtx {
        ws: &route.ws,
        rt: &shared.rt,
        group: shared.group,
        workspace,
        key: &shared.key,
        signing_key: &shared.signing_key,
    };
    handle_workspace_message(link, &ctx, &mut conn.session, msg).then_some(())
}

/// Peeks one frame's workspace and dispatches it: the link-level `Hello` if it peeked
/// [`LINK_WORKSPACE`], a routed workspace's own message if it peeked one this connection has open,
/// or a logged skip for a workspace this side never opened. `None` ends the connection.
fn dispatch_frame(
    link: &mut dyn Link,
    shared: &SharedCtx<'_>,
    conn: &mut Conn,
    frame: &txtodo_sync::Frame,
) -> Option<()> {
    let Some(workspace) = peek_workspace(&frame.body) else {
        log_frame_too_short_to_peek();
        return None;
    };
    if workspace == LINK_WORKSPACE {
        return dispatch_link_frame(frame, shared, conn);
    }
    dispatch_workspace_frame(link, frame, shared, conn, workspace)
}

/// Waits up to [`POLL`] for a frame: `Ok(None)` when none came, `Err(())` once the link is gone
/// (a real close, or a failure logged here).
fn recv_polled(link: &mut dyn Link) -> Result<Option<txtodo_sync::Frame>, ()> {
    match link.recv_timeout(POLL) {
        Ok(frame) => Ok(frame),
        Err(txtodo_sync::LinkError::Closed) => Err(()),
        Err(e) => {
            tracing::debug!(error = %e, "lan_session_recv_failed");
            Err(())
        }
    }
}

/// One turn: a frame if one arrives within [`POLL`], then a push/heartbeat/liveness tick
/// (`lan_session_live.rs`). `None` ends the connection.
fn turn(link: &mut dyn Link, shared: &SharedCtx<'_>, conn: &mut Conn) -> Option<()> {
    if shared.table.generation() != shared.generation {
        return log_routes_changed();
    }
    if let Some(frame) = recv_polled(link).ok()? {
        conn.live.heard();
        dispatch_frame(link, shared, conn, &frame)?;
    }
    let push = PushCtx {
        group: shared.group,
        key: &shared.key,
        signing_key: &shared.signing_key,
        routes: &shared.routes,
    };
    conn.live.tick(link, &push).then_some(())
}

/// Runs until the peer closes, goes silent, or the turn cap ends it (the dial side then
/// reconnects). Bounded like every loop here: a quiet turn is one [`POLL`], so the cap is hours.
fn run_shared_message_loop(link: &mut dyn Link, shared: &SharedCtx<'_>, conn: &mut Conn) {
    for _ in 0..MAX_TURNS_PER_SESSION {
        if turn(link, shared, conn).is_none() {
            return;
        }
    }
    tracing::warn!(cap = MAX_TURNS_PER_SESSION, "lan_session_turn_cap_reached");
}

/// A workspace opened or closed since this session greeted its set: end it, and the reconnect
/// greets the new set (task `sync-live-push`).
fn log_routes_changed() -> Option<()> {
    tracing::debug!("lan_session_routes_changed_reconnecting");
    None
}

/// Turns one connection runs before it is closed and redialed: at least a few hours at one
/// [`POLL`] each, and never fewer than the old per-session message cap.
const MAX_TURNS_PER_SESSION: usize = MAX_MESSAGES_PER_SESSION * 3;

/// Sends our own `Greet` for one workspace and, on success, the message it produced. A session-
/// level refusal (e.g. an already-greeted workspace) is logged and skipped, not fatal to the
/// connection's other workspaces — only a real send failure is.
fn send_greet_for(
    link: &mut dyn Link,
    shared: &SharedCtx<'_>,
    session: &mut Session,
    id: WorkspaceId,
) -> bool {
    match session.hello(id) {
        Ok(greet) => send_message(link, shared.group, id, &shared.key, greet).is_ok(),
        Err(e) => {
            tracing::warn!(error = %e, workspace = %id, "lan_session_greet_failed");
            true
        }
    }
}

/// Sends the once-per-connection link `Hello`, then a `Greet` for every workspace `shared.routes`
/// names. `false` (already logged) on a link handshake or send failure — nothing past that point
/// can matter.
fn send_initial_greetings(
    link: &mut dyn Link,
    shared: &SharedCtx<'_>,
    session: &mut Session,
) -> bool {
    let now_ms = any_route_now_ms(&shared.routes);
    let Ok(hello) = session.link_hello(now_ms) else {
        tracing::warn!("lan_session_link_hello_failed");
        return false;
    };
    if send_message(link, shared.group, LINK_WORKSPACE, &shared.key, hello).is_err() {
        return false;
    }
    shared
        .routes
        .keys()
        .all(|id| send_greet_for(link, shared, session, *id))
}

fn open_every_route(session: &mut Session, routes: &BTreeMap<WorkspaceId, WorkspaceRoute>) {
    for (id, route) in routes {
        if session.open_workspace(*id, read_heads(&route.ws)).is_err() {
            tracing::warn!(workspace = %id, "lan_session_open_workspace_failed");
        }
    }
}

fn log_no_routed_workspace() {
    tracing::debug!("lan_session_skipped_no_routed_workspace");
}

fn log_no_group_key() {
    tracing::debug!("lan_session_skipped_no_group_key");
}

/// The crypto-material half of [`build_shared_ctx`] — `None` (logged) when no routed workspace's
/// keystore holds the (necessarily shared, ADR 0021) group key, or the epoch table it builds
/// refuses it. Split out purely to keep `build_shared_ctx` under this workspace's cognitive-
/// complexity budget (`clippy.toml`).
fn group_crypto(first: &WorkspaceRoute) -> Option<(GroupKey, GroupKeys, DeviceSigningKey)> {
    let key = fetch_group_key(&first.ws)?;
    let keys = single_epoch_keys(key.clone())?;
    let signing_key = derive_group_op_signing_key(&key);
    Some((key, keys, signing_key))
}

/// Gathers `routes`' crypto material and routing table into one [`SharedCtx`] — `None` (logged)
/// when nothing is routed at all, or when the (necessarily shared, ADR 0021) group key cannot be
/// read from any routed workspace's keystore.
fn build_shared_ctx(routes: &WorkspaceRoutes, group: GroupId) -> Option<SharedCtx<'_>> {
    let generation = routes.generation();
    let all: BTreeMap<WorkspaceId, WorkspaceRoute> = routes.list().into_iter().collect();
    let Some(first) = all.values().next() else {
        log_no_routed_workspace();
        return None;
    };
    let Some((key, keys, signing_key)) = group_crypto(first) else {
        log_no_group_key();
        return None;
    };
    let live_peers = read(&first.ws).live_peers().clone();
    Some(SharedCtx {
        table: routes,
        generation,
        rt: Handle::current(),
        group,
        signing_key,
        key,
        keys,
        routes: all,
        live_peers,
    })
}

/// One full receive loop over an established connection, multiplexing every workspace `routes`
/// currently has registered for this `(device, group)` peer relationship — see
/// `lan_session_shared.rs`'s module doc for the wire sequence and failure scope. Runs on the
/// caller's own thread, which must be a blocking one (`Link::send`/`recv` block); returns when the
/// peer closes, the link handshake fails, or the `MAX_MESSAGES_PER_SESSION` bound is reached.
/// Returns `false` when the session bailed before its first greeting went out (no routed
/// workspace, no group key, or the greeting itself failed) — the dial side books that as a failed
/// dial so its backoff applies (`lan.rs::dial_and_spawn`).
pub(crate) fn drive_shared_session(
    link: &mut dyn Link,
    routes: &WorkspaceRoutes,
    device: DeviceId,
    group: GroupId,
) -> bool {
    let Some(shared) = build_shared_ctx(routes, group) else {
        return false;
    };
    // Deliberately at `info`, not `debug`: this is the one line proving stage 2's actual point —
    // that a peer relationship with more than one open workspace shares this single connection
    // rather than opening one per workspace (`tests/relay_multiplex.rs` greps for it). `workspaces
    // = 1` for the common single-workspace case is exactly as informative and equally cheap to
    // emit, so this is not gated on the count.
    tracing::info!(
        workspaces = shared.routes.len(),
        "lan_shared_session_started"
    );
    let mut session = Session::new(device, group);
    open_every_route(&mut session, &shared.routes);
    if !send_initial_greetings(link, &shared, &mut session) {
        return false;
    }
    let mut conn = Conn {
        session,
        live: Live::new(),
    };
    run_shared_message_loop(link, &shared, &mut conn);
    true
}
