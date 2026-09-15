//! Wires `txtodo_sync::FileCarrier` into a running `txtodod` (plan M8 `sync-file-carrier`,
//! `relay-converge-test`): periodically seals this device's not-yet-shared ops into its own
//! `sync/<device-id>.ops` file, and polls every *other* device's file for new frames to open,
//! verify and commit. `carrier.rs`'s own module doc calls this out as deliberately not built
//! there ("[i]mporting decoded ops into the store and deduping... is a different layer's job") —
//! this module is that layer.
//!
//! **One carrier per device, not per workspace** (task `daemon-shared-sync-link` stage 6):
//! `--sync-dir` is one daemon-wide setting (`WorkspaceOpenArgs`'s own doc), so
//! `FileCarrier::open(dir, device)` was always keyed by the *same* `(dir, device)` pair for every
//! open workspace — before this stage, each workspace's own `open_workspace_full` call opened its
//! own, byte-identical carrier handle to the same underlying file, redundantly (if safely, via the
//! already-landed `WrongWorkspace` AEAD check) re-polling and re-rejecting every *other* open
//! workspace's frames. [`DeviceFileCarrier`] is bound once, before any workspace opens (mirroring
//! `DeviceRelay::bind`); [`WorkspaceRoutes`] (reused verbatim from `device_relay.rs` — the same
//! table `control_dispatch.rs` routes incoming relay connections through) is what the one
//! consolidated tick loop below uses to send on behalf of every registered workspace and to route
//! an incoming frame's peeked `workspace_id` (`txtodo_sync::peek_workspace`) to the right one.
//!
//! **Broadcast-and-poll, not `Session`.** `lan.rs`/`relay.rs` drive a live two-way `Hello`/`Want`/
//! `Ops`/`Ack` handshake (`lan_session::drive_session`) because a QUIC connection has an actual
//! peer on the other end to negotiate with. A shared folder does not — there is nothing to
//! `Hello` and no live peer to ask a `Want` of. So this module skips `Session` entirely: every tick
//! it seals whatever of its own ops the *other* devices have not seen yet, per workspace (tracked
//! as `last_sent`, one `Heads` per workspace since consolidating means one tick now covers every
//! workspace sharing this `--sync-dir`, each with its own oplog — compared against the real heads
//! via `txtodo_sync::want`/`advance`, the exact same head-diffing primitive `Session`/`lan.rs` use,
//! just driven by "what did I last write" instead of a peer's `Want`) as one `Message::Ops` per
//! `serve_want`-produced batch, and separately polls for and imports whatever new frames appeared
//! in every other device's file. `lan_apply.rs`'s `serve_want`/`commit_incoming_ops`/
//! `device_keys_for` are reused as-is.
//!
//! **Own-file-only, for real.** `FileCarrier::poll`'s own `other_device_files` already filters out
//! this device's own file (`carrier.rs`'s doc: "each device appends only to its own file"), so the
//! "no self-loop" acceptance criterion (`tasks/relay-converge-test/todo.txt`) is a property of the
//! carrier this module wires in, not something this module has to re-implement.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use tokio::runtime::Handle;
use tokio::task::JoinHandle;
use txtodo_model::DeviceId;
use txtodo_store::WorkspaceId;
use txtodo_sync::{
    DevicePublicKey, FileCarrier, Frame, GroupId, GroupKey, GroupKeys, Heads, Link, Message,
    SealFor, advance, derive_group_op_signing_key, open as aead_open, peek_workspace,
    seal as aead_seal, verify_batch, want,
};

use crate::device_relay::{WorkspaceRoute, WorkspaceRoutes};
use crate::lan_apply::{commit_incoming_ops, device_keys_for, serve_want};
use crate::lan_session::{fetch_group_key, read_heads, single_epoch_keys};

/// How often the send/receive tick runs — the file-carrier counterpart of `lan.rs`'s
/// `RESYNC_INTERVAL`; short enough that this crate's own 30 s convergence deadline
/// (`relay-converge-test`) comfortably contains several ticks, long enough not to busy-poll disk.
const FILE_CARRIER_POLL_INTERVAL: Duration = Duration::from_millis(250);
/// `sealed_ops`/`Session` elsewhere in this crate always operate on the current group-key epoch;
/// the file carrier has no rotation-awareness of its own yet, same scope limit `lan_session.rs`'s
/// own `GROUP_EPOCH` constant already accepts.
const GROUP_EPOCH: u32 = 0;

/// This device's one file-carrier surface for a `--sync-dir`, shared by every open workspace
/// naming it. Bound once, before any workspace opens — mirrors `DeviceRelay`'s own device-level
/// (not per-workspace) lifetime.
pub struct DeviceFileCarrier {
    carrier: Mutex<FileCarrier>,
    routes: WorkspaceRoutes,
}

impl DeviceFileCarrier {
    /// Opens the carrier at `dir` for `device`. `None` (logged) on failure — matches this
    /// module's own pre-stage-6 "logged, never fatal" precedent.
    pub fn open(dir: PathBuf, device: DeviceId) -> Option<Arc<DeviceFileCarrier>> {
        match FileCarrier::open(dir, device) {
            Ok(carrier) => Some(Arc::new(DeviceFileCarrier {
                carrier: Mutex::new(carrier),
                routes: WorkspaceRoutes::new(),
            })),
            Err(e) => {
                tracing::warn!(error = %e, "file_carrier_open_failed_running_without_file_sync");
                None
            }
        }
    }

    /// The routing table an open workspace registers into, and the tick loop below reads from.
    pub fn routes(&self) -> &WorkspaceRoutes {
        &self.routes
    }

    fn lock(&self) -> MutexGuard<'_, FileCarrier> {
        self.carrier.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// The background file-carrier transport task; `abort()` on daemon shutdown, same pattern as
/// `LanTransport`/`RelayTransport`. One per device now, not one per workspace.
pub struct FileCarrierTransport {
    task: JoinHandle<()>,
}

impl FileCarrierTransport {
    /// Stops the file-carrier transport. Best-effort: the task may already have exited.
    pub fn abort(&self) {
        self.task.abort();
    }
}

/// Spawns the one consolidated send/receive tick loop for `device_file_carrier`. Called once, in
/// `main.rs::run`, right after `DeviceFileCarrier::open` succeeds — every open workspace shares
/// this same task via its registered route, none of them spawn their own.
pub fn start(device_file_carrier: Arc<DeviceFileCarrier>) -> FileCarrierTransport {
    FileCarrierTransport {
        task: tokio::spawn(run(device_file_carrier)),
    }
}

/// Seals `msg` (already signed by `serve_want`) whole under the group key — the file-carrier
/// counterpart of `lan_session.rs::send_message`, duplicated rather than shared because that
/// function takes a live `&mut dyn Link` argument shape this module's own call site does not have
/// at the same point (this module seals before it knows whether the carrier write will succeed).
fn seal_message(
    group: GroupId,
    workspace: WorkspaceId,
    key: &GroupKey,
    msg: &Message,
) -> Option<Frame> {
    let plain = msg.encode().ok()?;
    let for_ = SealFor {
        group,
        epoch: GROUP_EPOCH,
        workspace,
    };
    let sealed = aead_seal(plain.version, for_, key, &plain.body).ok()?;
    Some(Frame {
        version: plain.version,
        body: sealed,
    })
}

/// Sends `route`'s own not-yet-shared ops (per `last_sent`, this device's own record of what it
/// already wrote for *this* workspace — see the module doc) as one or more `Message::Ops` frames
/// appended to this device's own file. Advances `last_sent` only for a range that actually got
/// written, so a failed write is retried next tick rather than silently treated as delivered.
fn send_route(
    route: &WorkspaceRoute,
    workspace: WorkspaceId,
    key: &GroupKey,
    carrier: &mut FileCarrier,
    last_sent: &mut Heads,
) {
    let ranges = want(last_sent, &read_heads(&route.ws));
    if ranges.is_empty() {
        return;
    }
    let signing_key = derive_group_op_signing_key(key);
    let Ok(messages) = serve_want(&route.ws, &ranges, &signing_key) else {
        return;
    };
    for msg in &messages {
        let Message::Ops {
            ranges: msg_ranges, ..
        } = msg
        else {
            continue;
        };
        let Some(frame) = seal_message(route.group, workspace, key, msg) else {
            continue;
        };
        if carrier.send(frame).is_err() {
            continue;
        }
        for r in msg_ranges {
            let _ = advance(last_sent, r); // best-effort; a gap here just gets retried next tick
        }
    }
}

/// One send half of a tick: every workspace currently registered on `device_file_carrier`, each
/// against its own `last_sent` entry.
fn send_new_ops(
    device_file_carrier: &DeviceFileCarrier,
    key: &GroupKey,
    carrier: &mut FileCarrier,
    last_sent: &mut HashMap<WorkspaceId, Heads>,
) {
    for (workspace, route) in device_file_carrier.routes().list() {
        send_route(
            &route,
            workspace,
            key,
            carrier,
            last_sent.entry(workspace).or_default(),
        );
    }
}

/// Opens+decodes a frame; `None` on either failure, logged with which step (opening — a wrong or
/// unretained group key — vs. decoding — malformed bytes) so `recv_new_ops`'s caller can tell them
/// apart in the log without this function returning two error types.
fn open_and_decode(
    frame: &Frame,
    group: GroupId,
    workspace: WorkspaceId,
    keys: &GroupKeys,
) -> Option<Message> {
    let plain = aead_open(frame.version, group, workspace, keys, &frame.body)
        .inspect_err(|e| tracing::warn!(error = %e, "file_carrier_open_failed"))
        .ok()?;
    Message::decode(&Frame {
        version: frame.version,
        body: plain,
    })
    .inspect_err(|e| tracing::warn!(error = %e, "file_carrier_decode_failed"))
    .ok()
}

/// One incoming frame, opened, decoded and verified — `None` on anything that fails, already
/// logged; verification happens only after decode (so `device_keys_for` can see the actual ops),
/// the same order `lan_session.rs`'s own handling of a live `Session::on_ops` batch uses, and for
/// the same reason `sealed_ops::open_ops` cannot be reused here (its `device_keys` argument would
/// have to be known before the very decode that reveals which devices appear).
fn open_and_verify(
    frame: &Frame,
    group: GroupId,
    workspace: WorkspaceId,
    keys: &GroupKeys,
    verify_key: DevicePublicKey,
) -> Option<Vec<txtodo_model::Op>> {
    let Message::Ops {
        ops, signatures, ..
    } = open_and_decode(frame, group, workspace, keys)?
    else {
        return None;
    };
    let device_keys = device_keys_for(&ops, verify_key);
    verify_batch(&ops, &signatures, &device_keys)
        .inspect_err(|e| tracing::warn!(error = %e, "file_carrier_verify_failed"))
        .ok()?;
    Some(ops)
}

/// One incoming frame's peeked workspace id, resolved to the open workspace it's for — `None` for
/// a too-short frame or a workspace this device does not currently have open (a real, expected
/// case now: this file may carry frames for a workspace only *another* device has open, or one
/// this device has not opened yet). Split into one tiny function per step (mirrors
/// `control_dispatch.rs::route_first_frame`'s own split) purely to keep each one's own cognitive
/// complexity under this workspace's budget (`clippy.toml`).
fn route_frame(
    device_file_carrier: &DeviceFileCarrier,
    frame: &Frame,
) -> Option<(WorkspaceId, WorkspaceRoute)> {
    let workspace_id = peek_workspace_logged(frame)?;
    let route = route_logged(device_file_carrier, workspace_id)?;
    Some((workspace_id, route))
}

fn peek_workspace_logged(frame: &Frame) -> Option<WorkspaceId> {
    let id = peek_workspace(&frame.body);
    if id.is_none() {
        tracing::debug!("file_carrier_first_frame_too_short_dropping");
    }
    id
}

fn route_logged(
    device_file_carrier: &DeviceFileCarrier,
    workspace_id: WorkspaceId,
) -> Option<WorkspaceRoute> {
    let route = device_file_carrier.routes().route(workspace_id);
    if route.is_none() {
        tracing::debug!(%workspace_id, "file_carrier_unknown_workspace_dropping");
    }
    route
}

/// Polls every other device's file for new frames (via `FileCarrier::poll`, non-blocking, one
/// frame per call) and commits each one that opens and verifies, into whichever registered
/// workspace [`route_frame`] resolves it to. Own-file-only is the carrier's own property (module
/// doc); this loop only ever sees frames from *other* devices.
fn recv_new_ops(
    device_file_carrier: &DeviceFileCarrier,
    keys: &GroupKeys,
    verify_key: DevicePublicKey,
    carrier: &mut FileCarrier,
    rt: &Handle,
) {
    loop {
        let frame = match carrier.poll() {
            Ok(Some(frame)) => frame,
            Ok(None) => return,
            Err(e) => {
                tracing::warn!(error = %e, "file_carrier_poll_failed");
                return;
            }
        };
        let Some((workspace, route)) = route_frame(device_file_carrier, &frame) else {
            continue;
        };
        if let Some(ops) = open_and_verify(&frame, route.group, workspace, keys, verify_key) {
            commit_ops(&route.ws, rt, ops);
        }
    }
}

/// `commit_incoming_ops` calls `rt.block_on` internally (`lan_apply.rs::commit_one_file`) — safe
/// from `lan.rs`'s own call site because `drive_session` runs on a dedicated `spawn_blocking`
/// thread (`lan.rs::spawn_driver`), never a plain async worker. `recv_new_ops` runs directly inside
/// `run`'s plain `tokio::spawn`ed task instead (module doc: no per-connection thread to dedicate,
/// this is a periodic tick), so `block_in_place` is what makes a nested `block_on` legal here —
/// without it this panics with "Cannot start a runtime from within a runtime".
fn commit_ops(ws: &crate::server::SharedWorkspace, rt: &Handle, ops: Vec<txtodo_model::Op>) {
    tokio::task::block_in_place(|| commit_incoming_ops(ws, rt, ops));
}

/// One tick: `None` for the group key means no workspace is registered yet at all (nothing to
/// derive it from), same "quiet no-op" behaviour `file_carrier_alone_is_a_quiet_no_op` already
/// tests. The group key/signing/verify key are device-level (ADR 0021 — identical regardless of
/// which registered workspace they're read through), so this only needs to derive them once, from
/// any one registered route, then hands the whole table to `send_new_ops`/`recv_new_ops` to
/// iterate/route for real.
fn tick(
    device_file_carrier: &DeviceFileCarrier,
    rt: &Handle,
    last_sent: &mut HashMap<WorkspaceId, Heads>,
) {
    let Some((_, any_route)) = device_file_carrier.routes().list().into_iter().next() else {
        return;
    };
    let Some(key) = fetch_group_key(&any_route.ws) else {
        return;
    };
    let mut carrier = device_file_carrier.lock();
    send_new_ops(device_file_carrier, &key, &mut carrier, last_sent);
    let Some(keys) = single_epoch_keys(key.clone()) else {
        return;
    };
    let verify_key = derive_group_op_signing_key(&key).public_key();
    recv_new_ops(device_file_carrier, &keys, verify_key, &mut carrier, rt);
}

async fn run(device_file_carrier: Arc<DeviceFileCarrier>) {
    let rt = Handle::current();
    let mut last_sent: HashMap<WorkspaceId, Heads> = HashMap::new();
    let mut interval = tokio::time::interval(FILE_CARRIER_POLL_INTERVAL);
    loop {
        interval.tick().await;
        tick(&device_file_carrier, &rt, &mut last_sent);
    }
}
