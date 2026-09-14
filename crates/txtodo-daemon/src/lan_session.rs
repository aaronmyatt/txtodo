//! Driving one `Session` over one real `Link` connection (plan M4 `sync-lan-transport`,
//! daemon-wiring pass): sealing/opening every message with the workspace's group key, serving a
//! peer's `Want` from the local op log, and committing a peer's `Ops` batch through the right
//! `FileActor`. Split out of `lan.rs` for the file budget.
//!
//! **Scope of this pass.** Every message is sealed whole with the group key (confidentiality and
//! tamper-evidence for the batch as a unit — a flipped byte anywhere fails the AEAD tag). Individual
//! op *authorship* is not re-verified here: `Message::Ops` carries `Op` values with no accompanying
//! `Signature` (the wire format has no field for one, and `txtodo-store`'s own `ops.signature`
//! column is unpopulated today) — a real gap this pass does not close, flagged for
//! `sync-reject-tests`/`sync-crypto-envelope`. One convergence pass per connection: watching the
//! local store for new ops and re-greeting mid-connection is not wired this pass either — see
//! `CLAUDE.md` for both.

use std::fmt;
use std::sync::PoisonError;

use tokio::runtime::Handle;
use txtodo_model::{DeviceId, Op};
use txtodo_sync::{
    CryptoError, DeviceSigningKey, GroupId, GroupKey, GroupKeys, KeyId, Link, LinkError, Message,
    MessageError, OriginRange, Session, derive_group_op_signing_key, open, seal,
};

use crate::lan_apply::{commit_incoming_ops, serve_want};
use crate::server::SharedWorkspace;
use crate::workspace::Workspace;

/// Group-key epoch this pass always uses. Rotation (`sync-device-remove`) will need to make the
/// epoch a live value read off the session instead of a constant.
pub(crate) const GROUP_EPOCH: u32 = 0;

/// A hostile or buggy peer cannot keep one connection's driver looping forever — bounded the same
/// way every other loop in this codebase is.
pub(crate) const MAX_MESSAGES_PER_SESSION: usize = 100_000;

/// Why one sync exchange stopped. Every variant is handled the same way by the caller (log, close
/// the connection) — this exists to make the log line name what actually failed.
#[derive(Debug)]
enum SyncError {
    Link(LinkError),
    Message(MessageError),
    Crypto(CryptoError),
}

impl fmt::Display for SyncError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SyncError::Link(e) => write!(f, "link: {e}"),
            SyncError::Message(e) => write!(f, "message: {e}"),
            SyncError::Crypto(e) => write!(f, "crypto: {e}"),
        }
    }
}

impl From<LinkError> for SyncError {
    fn from(e: LinkError) -> SyncError {
        SyncError::Link(e)
    }
}
impl From<MessageError> for SyncError {
    fn from(e: MessageError) -> SyncError {
        SyncError::Message(e)
    }
}
impl From<CryptoError> for SyncError {
    fn from(e: CryptoError) -> SyncError {
        SyncError::Crypto(e)
    }
}

pub(crate) fn read(ws: &SharedWorkspace) -> std::sync::RwLockReadGuard<'_, Workspace> {
    ws.read().unwrap_or_else(PoisonError::into_inner)
}

pub(crate) fn write(ws: &SharedWorkspace) -> std::sync::RwLockWriteGuard<'_, Workspace> {
    ws.write().unwrap_or_else(PoisonError::into_inner)
}

/// `pub(crate)`: `file_carrier.rs` (plan M8 `relay-converge-test`) reuses this too — the file
/// carrier's send/receive path needs the same group key `drive_session` does, with no `Session` of
/// its own (see that module's doc for why it does not reuse `drive_session` wholesale).
pub(crate) fn fetch_group_key(ws: &SharedWorkspace) -> Option<GroupKey> {
    let bytes = read(ws).key_store().get(KeyId::Group(GROUP_EPOCH)).ok()??;
    let array: [u8; txtodo_sync::KEY_BYTES] = bytes.expose().try_into().ok()?;
    Some(GroupKey::from_bytes(array))
}

/// `pub(crate)`: see [`fetch_group_key`]'s doc.
pub(crate) fn single_epoch_keys(key: GroupKey) -> Option<GroupKeys> {
    let mut keys = GroupKeys::new();
    keys.insert(GROUP_EPOCH, key).ok()?;
    Some(keys)
}

/// `pub(crate)`: see [`fetch_group_key`]'s doc.
pub(crate) fn read_heads(ws: &SharedWorkspace) -> txtodo_sync::Heads {
    let store = read(ws).store().clone();
    store
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .heads()
        .unwrap_or_default()
}

/// Seals `msg` whole under the group key and sends it. The AEAD header carries `group`/`epoch` in
/// the clear (`seal`'s own doc); the sealed bytes become the outer `Frame`'s body.
fn send_message(
    link: &mut dyn Link,
    group: GroupId,
    key: &GroupKey,
    msg: Message,
) -> Result<(), SyncError> {
    let plain = msg.encode()?;
    let sealed = seal(plain.version, group, GROUP_EPOCH, key, &plain.body)?;
    link.send(txtodo_sync::Frame {
        version: plain.version,
        body: sealed,
    })?;
    Ok(())
}

/// Receives one frame, opens it under `keys`, and decodes the `Message` inside.
fn recv_message(
    link: &mut dyn Link,
    group: GroupId,
    keys: &GroupKeys,
) -> Result<Message, SyncError> {
    let frame = link.recv()?;
    let plain = open(frame.version, group, keys, &frame.body)?;
    Ok(Message::decode(&txtodo_sync::Frame {
        version: frame.version,
        body: plain,
    })?)
}

/// `None` on any failure worth ending the connection over — already logged.
fn recv_next(link: &mut dyn Link, group: GroupId, keys: &GroupKeys) -> Option<Message> {
    match recv_message(link, group, keys) {
        Ok(msg) => Some(msg),
        Err(SyncError::Link(LinkError::Closed)) => None,
        Err(e) => {
            tracing::debug!(error = %e, "lan_session_recv_failed");
            None
        }
    }
}

/// Everything a message handler needs besides the link and the session itself — bundled so no
/// handler function needs more than `maxParams` arguments.
struct SessionCtx<'a> {
    ws: &'a SharedWorkspace,
    rt: &'a Handle,
    group: GroupId,
    key: &'a GroupKey,
    /// Stand-in op-signing key derived from `key` — see the module doc's "Scope of this pass" and
    /// `txtodo_sync::lan_op_signing` for exactly what it does and does not prove.
    signing_key: DeviceSigningKey,
}

impl SessionCtx<'_> {
    fn send(&self, link: &mut dyn Link, msg: Message) -> Result<(), SyncError> {
        send_message(link, self.group, self.key, msg)
    }
}

fn handle_hello(
    link: &mut dyn Link,
    ctx: &SessionCtx<'_>,
    session: &mut Session,
    msg: &Message,
) -> bool {
    let now_ms = read(ctx.ws).clock().now_ms();
    let greeting = match session.on_hello(msg, now_ms) {
        Ok(g) => g,
        Err(e) => {
            tracing::warn!(error = %e, "lan_hello_refused");
            return false;
        }
    };
    ctx.send(link, greeting.want).is_ok()
}

fn handle_want(link: &mut dyn Link, ctx: &SessionCtx<'_>, ranges: &[OriginRange]) -> bool {
    let batches = match serve_want(ctx.ws, ranges, &ctx.signing_key) {
        Ok(b) => b,
        Err(e) => {
            tracing::warn!(error = %e, "lan_serve_want_failed");
            return false;
        }
    };
    for batch in batches {
        if ctx.send(link, batch).is_err() {
            return false;
        }
    }
    true
}

fn ops_or_refuse(ctx: &SessionCtx<'_>, session: &mut Session, msg: &Message) -> Option<Vec<Op>> {
    // An empty map when `msg` is not actually `Ops` is fine: `on_ops` checks the variant before
    // it ever consults `device_keys` and reports "unexpected message" instead.
    let device_keys = match msg {
        Message::Ops { ops, .. } => {
            crate::lan_apply::device_keys_for(ops, ctx.signing_key.public_key())
        }
        _ => std::collections::BTreeMap::new(),
    };
    match session.on_ops(msg, &device_keys) {
        Ok(ops) => Some(ops),
        Err(e) => {
            tracing::warn!(error = %e, "lan_ops_refused");
            None
        }
    }
}

fn commit_and_ack(
    ctx: &SessionCtx<'_>,
    session: &mut Session,
    ops: Vec<Op>,
    ranges: Vec<OriginRange>,
) -> Option<Message> {
    let committed_ranges = if commit_incoming_ops(ctx.ws, ctx.rt, ops) {
        ranges
    } else {
        Vec::new()
    };
    match session.committed(&committed_ranges) {
        Ok(ack) => Some(ack),
        Err(e) => {
            tracing::warn!(error = %e, "lan_ack_refused");
            None
        }
    }
}

fn handle_ops(
    link: &mut dyn Link,
    ctx: &SessionCtx<'_>,
    session: &mut Session,
    msg: &Message,
    ranges: Vec<OriginRange>,
) -> bool {
    let Some(ops) = ops_or_refuse(ctx, session, msg) else {
        return false;
    };
    let Some(ack) = commit_and_ack(ctx, session, ops, ranges) else {
        return false;
    };
    ctx.send(link, ack).is_ok()
}

/// One received message, dispatched by variant. `false` ends the connection.
fn handle_message(
    link: &mut dyn Link,
    ctx: &SessionCtx<'_>,
    session: &mut Session,
    msg: Message,
) -> bool {
    match &msg {
        Message::Hello { .. } => handle_hello(link, ctx, session, &msg),
        Message::Want { ranges } => handle_want(link, ctx, ranges),
        Message::Ops { ranges, .. } => handle_ops(link, ctx, session, &msg, ranges.clone()),
        Message::Ack { committed } => {
            tracing::debug!(runs = committed.len(), "lan_peer_acked");
            true
        }
    }
}

fn initial_hello(session: &mut Session, ws: &SharedWorkspace) -> Option<Message> {
    let now_ms = read(ws).clock().now_ms();
    session.hello(now_ms).ok()
}

fn run_message_loop(
    link: &mut dyn Link,
    ctx: &SessionCtx<'_>,
    keys: &GroupKeys,
    session: &mut Session,
) {
    for _ in 0..MAX_MESSAGES_PER_SESSION {
        let Some(msg) = recv_next(link, ctx.group, keys) else {
            return;
        };
        if !handle_message(link, ctx, session, msg) {
            return;
        }
    }
    tracing::warn!(
        cap = MAX_MESSAGES_PER_SESSION,
        "lan_session_message_cap_reached"
    );
}

/// One full receive loop over an established connection: send our `Hello`, then react to whatever
/// the peer sends — its own `Hello` (reply with `Want`), a `Want` (serve it from the store), an
/// `Ops` batch (commit it, ack what actually landed), or an `Ack` (logged only — see the module
/// doc on why this pass does not retry from it). Runs on the caller's own thread, which must be a
/// blocking one (`Link::send`/`recv` block); returns when the peer closes, refuses, or the
/// `MAX_MESSAGES_PER_SESSION` bound is reached.
pub(crate) fn drive_session(
    link: &mut dyn Link,
    ws: SharedWorkspace,
    device: DeviceId,
    group: GroupId,
) {
    let Some(key) = fetch_group_key(&ws) else {
        tracing::debug!("lan_session_skipped_no_group_key");
        return;
    };
    let Some(keys) = single_epoch_keys(key.clone()) else {
        return;
    };
    let rt = Handle::current();
    let signing_key = derive_group_op_signing_key(&key);
    let ctx = SessionCtx {
        ws: &ws,
        rt: &rt,
        group,
        key: &key,
        signing_key,
    };
    let mut session = Session::new(device, group, read_heads(&ws));
    let Some(hello) = initial_hello(&mut session, &ws) else {
        return;
    };
    if ctx.send(link, hello).is_err() {
        return;
    }
    run_message_loop(link, &ctx, &keys, &mut session);
}
