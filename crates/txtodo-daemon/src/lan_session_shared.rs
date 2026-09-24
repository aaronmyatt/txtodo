//! The real stage-2 wiring for task `daemon-workspace-session-multiplex`: one `Session` per peer
//! connection, driving however many of that peer relationship's open, routed workspaces share it
//! — interleaving outgoing `Greet`/`Want`/`Ops`/`Ack` across all of them, and demuxing every
//! incoming frame by its own peeked `workspace` id (`txtodo_sync::peek_workspace`) rather than
//! routing the whole connection to one workspace up front. Split out of `lan_session.rs` for that
//! file's line budget, and split again into this file (crypto/wire primitives + per-workspace
//! message handlers) and `lan_session_dispatch.rs` (the actual read loop and entry point) for its
//! own budget — `lan_session.rs` keeps the crypto-material lookups (`fetch_group_key`/
//! `read_heads`/etc.) both this file and `file_carrier.rs` share.
//!
//! **Wire sequence.** Every connection starts with exactly one link-level `Message::Hello`
//! (`Session::link_hello`/`on_link_hello` — device+group+protocol+skew, unchanged from before this
//! stage) sealed under [`LINK_WORKSPACE`], a reserved sentinel id, since the AEAD layer still binds
//! exactly one `workspace` per sealed frame (`SealFor`) and the link-level handshake belongs to no
//! single workspace. Once that is sent, this side also sends a `Message::Greet` for every locally
//! open, routed workspace (`WorkspaceRoutes::list()`), each sealed under its own real workspace id.
//! `lan_session_dispatch.rs`'s read loop then processes whatever arrives, in whatever order the
//! peer sent it: a peeked workspace of [`LINK_WORKSPACE`] is the link `Hello`; anything else is
//! looked up in this connection's own routing table and, if found, opened under that specific
//! workspace and handed to this file's [`handle_workspace_message`]. A peeked workspace this side
//! has no route for (the peer has a workspace open that we don't) is logged and skipped, not
//! fatal — the whole point of sharing one connection is that one workspace's absence or refusal
//! must not stop every other workspace on the same link from converging.
//!
//! **Failure scope.** A link-level `Hello` failure (foreign group/protocol, peer clock too far
//! ahead) or a real transport/decrypt failure ends the whole connection, same as before this
//! stage — those are properties of the *link*, not any one workspace. A single workspace's own
//! session-level refusal (an out-of-order message, a stray range) is logged and that workspace's
//! own state is left untouched, but the connection itself keeps running for every other workspace
//! — a deliberate change from the pre-multiplex `drive_session`, where the only workspace *was*
//! the whole connection, so ending on its refusal and ending the connection were the same thing.
//! One exception since sessions became long-lived (task `sync-live-push`): a refused `Ops` batch
//! ends the connection, because a workspace left out of step would otherwise stay stuck until the
//! session's natural end; the reconnect's fresh `Greet`/`Want` puts it right. Except a batch that
//! only fails to follow our heads (task `sync-ack-before-held`, 2026-09-25): it is skipped, the
//! connection carries on, and the sender, which counts a run as held only once we ack it
//! (`lan_session_live.rs`), sends it again.

use std::fmt;

use tokio::runtime::Handle;
use txtodo_model::Ulid;
use txtodo_store::WorkspaceId;
use txtodo_sync::{
    CryptoError, DeviceSigningKey, Frame, GroupId, GroupKey, GroupKeys, Link, LinkError, Message,
    MessageError, SealFor, Session, open, seal,
};

use crate::lan_session::GROUP_EPOCH;

/// A hostile or buggy peer cannot keep one connection's driver looping forever — bounded the same
/// way every other loop in this codebase is. Shared across every workspace this connection
/// multiplexes, not per-workspace: it bounds the connection's total message count.
pub(crate) const MAX_MESSAGES_PER_SESSION: usize = 100_000;

/// Reserved workspace id used only to bind the link-level `Hello` frame's AEAD header — never a
/// real registered workspace. Every real `WorkspaceId` is minted from a ULID with a non-zero
/// timestamp component (`txtodo_store::WorkspaceId`/`Ulid::new`), so the all-zero id used here can
/// never collide with one.
pub(crate) const LINK_WORKSPACE: WorkspaceId = WorkspaceId::new(Ulid::from_u128(0));

/// Why one sync exchange stopped. Every variant is handled the same way by the caller (log, close
/// the connection) — this exists to make the log line name what actually failed.
#[derive(Debug)]
pub(crate) enum SyncError {
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

/// Seals `msg` whole under the group key for `workspace` and sends it — `workspace` is
/// [`LINK_WORKSPACE`] for the once-per-connection `Hello`, or a real one for everything else.
/// `pub(crate)`: `lan_session_dispatch.rs` sends the initial `Hello`/`Greet`s through this too.
pub(crate) fn send_message(
    link: &mut dyn Link,
    group: GroupId,
    workspace: WorkspaceId,
    key: &GroupKey,
    msg: Message,
) -> Result<(), SyncError> {
    let plain = msg.encode()?;
    let for_ = SealFor {
        group,
        epoch: GROUP_EPOCH,
        workspace,
    };
    let sealed = seal(plain.version, for_, key, &plain.body)?;
    link.send(Frame {
        version: plain.version,
        body: sealed,
    })?;
    Ok(())
}

/// Opens `frame` under `workspace`'s key material and decodes the `Message` inside.
fn open_and_decode(
    frame: &Frame,
    group: GroupId,
    workspace: WorkspaceId,
    keys: &GroupKeys,
) -> Result<Message, SyncError> {
    let plain = open(frame.version, group, workspace, keys, &frame.body)?;
    Ok(Message::decode(&Frame {
        version: frame.version,
        body: plain,
    })?)
}

/// `None` on any failure worth ending the connection over — already logged. `pub(crate)`: the
/// dispatch loop in `lan_session_dispatch.rs` calls this.
pub(crate) fn open_and_decode_logged(
    frame: &Frame,
    group: GroupId,
    workspace: WorkspaceId,
    keys: &GroupKeys,
) -> Option<Message> {
    match open_and_decode(frame, group, workspace, keys) {
        Ok(msg) => Some(msg),
        Err(e) => {
            tracing::debug!(error = %e, kind = sync_error_kind(&e), "lan_session_open_failed");
            None
        }
    }
}

/// A stable, queryable tag for `SyncError`'s own inner variant — previously only the free-text
/// `Display` string distinguished a `WrongGroup`/`WrongWorkspace`/`UnknownEpoch`/`Decrypt`/etc.
/// crypto refusal from a `MessageError`. Re-derives `CryptoError`'s own shape rather than calling
/// `txtodo_sync::CryptoError::kind()` (that method is `pub(crate)` to that crate only).
fn sync_error_kind(e: &SyncError) -> &'static str {
    match e {
        SyncError::Link(_) => "link",
        SyncError::Message(m) => message_error_kind(m),
        SyncError::Crypto(c) => crypto_error_kind(c),
    }
}

fn message_error_kind(e: &MessageError) -> &'static str {
    match e {
        MessageError::Frame(_) => "frame",
        MessageError::TooMany { .. } => "too_many",
        MessageError::BackwardsRange(_) => "backwards_range",
        MessageError::SignatureCount { .. } => "signature_count",
        MessageError::Codec(_) => "codec",
        MessageError::TrailingBytes(_) => "trailing_bytes",
    }
}

fn crypto_error_kind(e: &CryptoError) -> &'static str {
    match e {
        CryptoError::Encode(_) => "encode",
        CryptoError::BatchLength { .. } => "batch_length",
        CryptoError::UnknownDevice { .. } => "unknown_device",
        CryptoError::BadPublicKey { .. } => "bad_public_key",
        CryptoError::SignatureInvalid { .. } => "signature_invalid",
        CryptoError::WrongVersion { .. } => "wrong_version",
        CryptoError::WrongGroup { .. } => "wrong_group",
        CryptoError::WrongWorkspace { .. } => "wrong_workspace",
        CryptoError::Truncated { .. } => "truncated",
        CryptoError::UnknownEpoch { .. } => "unknown_epoch",
        CryptoError::TooManyEpochs { .. } => "too_many_epochs",
        CryptoError::Encrypt => "encrypt",
        CryptoError::Decrypt { .. } => "decrypt",
        CryptoError::Entropy => "entropy",
    }
}

/// Everything a per-workspace message handler needs besides the link and the session itself —
/// bundled so no handler function needs more than `maxParams` arguments. Built fresh
/// (`lan_session_dispatch.rs`) for whichever workspace the current incoming message peeked to;
/// `key`/`signing_key` are the same for every workspace on this connection (one shared group key
/// per device-set, ADR 0021), so they are borrowed from the connection-wide `SharedCtx` rather
/// than recomputed. `pub(crate)` fields: constructed one file over.
pub(crate) struct SessionCtx<'a> {
    pub(crate) ws: &'a crate::server::SharedWorkspace,
    pub(crate) rt: &'a Handle,
    pub(crate) group: GroupId,
    pub(crate) workspace: WorkspaceId,
    pub(crate) key: &'a GroupKey,
    pub(crate) signing_key: &'a DeviceSigningKey,
}

impl SessionCtx<'_> {
    pub(crate) fn send(&self, link: &mut dyn Link, msg: Message) -> Result<(), SyncError> {
        send_message(link, self.group, self.workspace, self.key, msg)
    }
}

fn log_link_hello_accepted(skew: txtodo_model::Skew) {
    tracing::debug!(?skew, "lan_link_hello_accepted");
}

fn log_link_hello_refused(e: &txtodo_sync::SessionError) {
    tracing::warn!(error = %e, "lan_link_hello_refused");
}

pub(crate) fn handle_link_hello(session: &mut Session, msg: &Message, now_ms: u64) -> bool {
    match session.on_link_hello(msg, now_ms) {
        Ok(skew) => {
            log_link_hello_accepted(skew);
            true
        }
        Err(e) => {
            log_link_hello_refused(&e);
            false
        }
    }
}

/// `Greeted -> Wanting | Idle`: a peer's `Greet` for one workspace, replied to with our own
/// `Want`. A session-level refusal (out-of-order, wrong workspace) is logged and this workspace's
/// state is left untouched, but the *connection* keeps running — the module doc's "failure scope".
fn handle_greet(
    link: &mut dyn Link,
    ctx: &SessionCtx<'_>,
    session: &mut Session,
    msg: &Message,
) -> bool {
    let want = match session.on_hello(ctx.workspace, msg) {
        Ok(w) => w,
        Err(e) => {
            tracing::warn!(error = %e, workspace = %ctx.workspace, "lan_greet_refused");
            return true;
        }
    };
    match ctx.send(link, want) {
        Ok(()) => true,
        Err(e) => log_want_send_failed(ctx.workspace, &e),
    }
}

fn log_want_send_failed(workspace: WorkspaceId, e: &SyncError) -> bool {
    tracing::warn!(error = %e, %workspace, "lan_want_send_failed");
    false
}

fn log_peer_acked(runs: usize, workspace: WorkspaceId) {
    tracing::debug!(runs, workspace = %workspace, "lan_peer_acked");
}

fn log_hello_on_workspace_frame(workspace: WorkspaceId) {
    tracing::debug!(
        workspace = %workspace,
        "lan_session_hello_sealed_for_a_real_workspace_ignoring"
    );
}

/// One received message already known to belong to `ctx.workspace`, dispatched by variant.
/// `false` ends the connection — only a real `Link` send failure does that here; every
/// session-level refusal is handled (and logged) inside its own handler above. `pub(crate)`:
/// `lan_session_dispatch.rs`'s read loop calls this once it has resolved `ctx`.
pub(crate) fn handle_workspace_message(
    link: &mut dyn Link,
    ctx: &SessionCtx<'_>,
    session: &mut Session,
    msg: Message,
) -> bool {
    match &msg {
        Message::Greet { .. } => handle_greet(link, ctx, session, &msg),
        // Served by `lan_session_live.rs`, a batch a turn (task sync-link-fairness).
        Message::Want { .. } => true,
        Message::Ops { ranges, .. } => {
            crate::lan_session_ops::handle_ops(link, ctx, session, &msg, ranges.clone())
        }
        Message::Ack { committed, .. } => {
            log_peer_acked(committed.len(), ctx.workspace);
            true
        }
        Message::Hello { .. } => {
            log_hello_on_workspace_frame(ctx.workspace);
            true
        }
    }
}
