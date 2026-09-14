//! Wires `txtodo_sync::FileCarrier` into a running `txtodod` (plan M8 `sync-file-carrier`,
//! `relay-converge-test`): periodically seals this device's not-yet-shared ops into its own
//! `sync/<device-id>.ops` file, and polls every *other* device's file for new frames to open,
//! verify and commit. `carrier.rs`'s own module doc calls this out as deliberately not built
//! there ("[i]mporting decoded ops into the store and deduping... is a different layer's job") —
//! this module is that layer, the daemon-level `sync-file-carrier` placement note never landed
//! until now (`--sync-dir` has existed in `txtodo-cli`'s config since that task, but nothing on
//! the daemon side ever opened a `FileCarrier`).
//!
//! **Broadcast-and-poll, not `Session`.** `lan.rs`/`relay.rs` drive a live two-way `Hello`/`Want`/
//! `Ops`/`Ack` handshake (`lan_session::drive_session`) because a QUIC connection has an actual
//! peer on the other end to negotiate with. A shared folder does not — there is nothing to
//! `Hello` and no live peer to ask a `Want` of. So this module skips `Session` entirely: every tick
//! it seals whatever of its own ops the *other* devices have not seen yet (tracked as `last_sent`,
//! a local `Heads` compared against the real one via `txtodo_sync::want`/`advance`, the exact same
//! head-diffing primitive `Session`/`lan.rs` use, just driven by "what did I last write" instead of
//! a peer's `Want`) as one `Message::Ops` per `serve_want`-produced batch, and separately polls for
//! and imports whatever new frames appeared in every other device's file. `lan_apply.rs`'s
//! `serve_want`/`commit_incoming_ops`/`device_keys_for` are reused as-is — this module wires a new
//! carrier around code that already exists, not a second copy of it.
//!
//! **Own-file-only, for real.** `FileCarrier::poll`'s own `other_device_files` already filters out
//! this device's own file (`carrier.rs`'s doc: "each device appends only to its own file"), so the
//! "no self-loop" acceptance criterion (`tasks/relay-converge-test/todo.txt`) is a property of the
//! carrier this module wires in, not something this module has to re-implement.

use std::path::PathBuf;
use std::time::Duration;

use tokio::runtime::Handle;
use tokio::task::JoinHandle;
use txtodo_sync::{
    FileCarrier, GroupId, GroupKey, Heads, Link, Message, advance, derive_group_op_signing_key,
    open as aead_open, seal as aead_seal, verify_batch, want,
};

use crate::lan_apply::{commit_incoming_ops, device_keys_for, serve_want};
use crate::lan_session::{GROUP_EPOCH, fetch_group_key, read, read_heads, single_epoch_keys};
use crate::server::SharedWorkspace;

/// How often the send/receive tick runs — the file-carrier counterpart of `lan.rs`'s
/// `RESYNC_INTERVAL`; short enough that this crate's own 30 s convergence deadline
/// (`relay-converge-test`) comfortably contains several ticks, long enough not to busy-poll disk.
const FILE_CARRIER_POLL_INTERVAL: Duration = Duration::from_millis(250);

/// The background file-carrier transport task; `abort()` on daemon shutdown, same pattern as
/// `LanTransport`/`RelayTransport`.
pub struct FileCarrierTransport {
    task: JoinHandle<()>,
}

impl FileCarrierTransport {
    /// Stops the file-carrier transport. Best-effort: the task may already have exited (the
    /// carrier's directory could not be opened).
    pub fn abort(&self) {
        self.task.abort();
    }
}

/// Starts the file-carrier transport as a background task when `sync_dir` names one; `None`
/// (`--sync-dir` omitted) spawns nothing at all, matching `relay.rs::start`'s own "additive,
/// nothing configured means nothing runs" shape.
pub fn start(ws: SharedWorkspace, sync_dir: Option<PathBuf>) -> Option<FileCarrierTransport> {
    let dir = sync_dir?;
    Some(FileCarrierTransport {
        task: tokio::spawn(run(ws, dir)),
    })
}

async fn open_carrier(ws: &SharedWorkspace, dir: PathBuf) -> Option<FileCarrier> {
    let device = read(ws).device();
    match FileCarrier::open(dir, device) {
        Ok(carrier) => Some(carrier),
        Err(e) => {
            tracing::warn!(error = %e, "file_carrier_open_failed_running_without_file_sync");
            None
        }
    }
}

/// Seals `msg` (already signed by `serve_want`) whole under the group key — the file-carrier
/// counterpart of `lan_session.rs::send_message`, duplicated rather than shared because that
/// function takes a live `&mut dyn Link` argument shape this module's own call site does not have
/// at the same point (this module seals before it knows whether the carrier write will succeed).
fn seal_message(group: GroupId, key: &GroupKey, msg: &Message) -> Option<txtodo_sync::Frame> {
    let plain = msg.encode().ok()?;
    let sealed = aead_seal(plain.version, group, GROUP_EPOCH, key, &plain.body).ok()?;
    Some(txtodo_sync::Frame {
        version: plain.version,
        body: sealed,
    })
}

/// Sends every not-yet-shared op (per `last_sent`, this task's own record of what it already wrote
/// — see the module doc) as one or more `Message::Ops` frames appended to this device's own file.
/// Advances `last_sent` only for a range that actually got written, so a failed write is retried
/// next tick rather than silently treated as delivered.
fn send_new_ops(ws: &SharedWorkspace, carrier: &mut FileCarrier, last_sent: &mut Heads) {
    let Some(key) = fetch_group_key(ws) else {
        return;
    };
    let group = read(ws).group();
    let ranges = want(last_sent, &read_heads(ws));
    if ranges.is_empty() {
        return;
    }
    let signing_key = derive_group_op_signing_key(&key);
    let Ok(messages) = serve_want(ws, &ranges, &signing_key) else {
        return;
    };
    for msg in &messages {
        let Message::Ops {
            ranges: msg_ranges, ..
        } = msg
        else {
            continue;
        };
        let Some(frame) = seal_message(group, &key, msg) else {
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

/// Opens+decodes a frame; `None` on either failure, logged with which step (opening — a wrong or
/// unretained group key — vs. decoding — malformed bytes) so `recv_new_ops`'s caller can tell them
/// apart in the log without this function returning two error types.
fn open_and_decode(
    frame: &txtodo_sync::Frame,
    group: GroupId,
    keys: &txtodo_sync::GroupKeys,
) -> Option<Message> {
    let plain = aead_open(frame.version, group, keys, &frame.body)
        .inspect_err(|e| tracing::warn!(error = %e, "file_carrier_open_failed"))
        .ok()?;
    Message::decode(&txtodo_sync::Frame {
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
    frame: &txtodo_sync::Frame,
    group: GroupId,
    keys: &txtodo_sync::GroupKeys,
    verify_key: txtodo_sync::DevicePublicKey,
) -> Option<Vec<txtodo_model::Op>> {
    let Message::Ops {
        ops, signatures, ..
    } = open_and_decode(frame, group, keys)?
    else {
        return None;
    };
    let device_keys = device_keys_for(&ops, verify_key);
    verify_batch(&ops, &signatures, &device_keys)
        .inspect_err(|e| tracing::warn!(error = %e, "file_carrier_verify_failed"))
        .ok()?;
    Some(ops)
}

/// Polls every other device's file for new frames (via `FileCarrier::poll`, non-blocking, one
/// frame per call) and commits each one that opens and verifies. Own-file-only is the carrier's
/// own property (module doc); this loop only ever sees frames from *other* devices.
fn recv_new_ops(ws: &SharedWorkspace, carrier: &mut FileCarrier, rt: &Handle) {
    let Some(key) = fetch_group_key(ws) else {
        return;
    };
    let Some(keys) = single_epoch_keys(key.clone()) else {
        return;
    };
    let group = read(ws).group();
    let verify_key = derive_group_op_signing_key(&key).public_key();
    loop {
        let frame = match carrier.poll() {
            Ok(Some(frame)) => frame,
            Ok(None) => return,
            Err(e) => {
                tracing::warn!(error = %e, "file_carrier_poll_failed");
                return;
            }
        };
        if let Some(ops) = open_and_verify(&frame, group, &keys, verify_key) {
            commit_ops(ws, rt, ops);
        }
    }
}

/// `commit_incoming_ops` calls `rt.block_on` internally (`lan_apply.rs::commit_one_file`) — safe
/// from `lan.rs`'s own call site because `drive_session` runs on a dedicated `spawn_blocking`
/// thread (`lan.rs::spawn_driver`), never a plain async worker. `recv_new_ops` runs directly inside
/// `run`'s plain `tokio::spawn`ed task instead (module doc: no per-connection thread to dedicate,
/// this is a periodic tick), so `block_in_place` is what makes a nested `block_on` legal here —
/// without it this panics with "Cannot start a runtime from within a runtime".
fn commit_ops(ws: &SharedWorkspace, rt: &Handle, ops: Vec<txtodo_model::Op>) {
    tokio::task::block_in_place(|| commit_incoming_ops(ws, rt, ops));
}

async fn run(ws: SharedWorkspace, dir: PathBuf) {
    let Some(mut carrier) = open_carrier(&ws, dir).await else {
        return;
    };
    let rt = Handle::current();
    let mut last_sent: Heads = Heads::new();
    let mut interval = tokio::time::interval(FILE_CARRIER_POLL_INTERVAL);
    loop {
        interval.tick().await;
        send_new_ops(&ws, &mut carrier, &mut last_sent);
        recv_new_ops(&ws, &mut carrier, &rt);
    }
}
