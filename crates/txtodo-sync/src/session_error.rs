//! Why a `Session` refused a message or a caller action. Split from `session.rs` for the file
//! budget; every variant names what was seen and what was expected (CLAUDE.md §3).

use std::fmt;

use txtodo_store::WorkspaceId;

use crate::crypto_error::CryptoError;
use crate::message::{GroupId, OriginRange};
use crate::session::SessionState;
use crate::want::Gap;

/// Why a message or a caller action is not legal now.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SessionError {
    /// This message or action is not allowed in the current state.
    Unexpected {
        /// The state we were in.
        state: SessionState,
        /// What arrived.
        what: &'static str,
    },
    /// A caller named a workspace this `Session` never opened (`Session::open_workspace`) — or
    /// one already closed. Never a panic: an unopened workspace is exactly as routine as a stray
    /// wire message, since a peer or a caller can name one this session simply does not know.
    UnknownWorkspace(WorkspaceId),
    /// `Session::open_workspace` would exceed `MAX_OPEN_WORKSPACES`; re-opening an
    /// already-open id never hits this (see that method's doc).
    TooManyWorkspaces {
        /// How many would be open, including the new one.
        len: usize,
        /// The cap.
        max: usize,
    },
    /// A `Want`/`Ops`/`Ack`'s own embedded `workspace` field does not match the workspace the
    /// caller named when calling `Session::on_ops` — never silently routed to the wrong
    /// sub-session.
    WorkspaceMismatch {
        /// What the caller asked to route to.
        called: WorkspaceId,
        /// What the message itself carries.
        message: WorkspaceId,
    },
    /// The peer is in another sync group.
    GroupMismatch {
        /// Ours.
        ours: GroupId,
        /// Theirs.
        theirs: GroupId,
    },
    /// The peer speaks another application protocol.
    ProtocolMismatch {
        /// Ours.
        ours: u16,
        /// Theirs.
        theirs: u16,
    },
    /// The peer's clock is too far ahead; refused before any op (see `Hlc::merge`).
    PeerAhead {
        /// Its wall clock.
        peer_ms: u64,
        /// Ours.
        local_ms: u64,
        /// By how much it leads.
        lead_ms: u64,
    },
    /// `Ops` covered a run we never asked for.
    Unrequested(OriginRange),
    /// `committed()` named a run that was not in the in-flight batch.
    NotInBatch(OriginRange),
    /// A committed run does not follow the head we hold.
    Gap(Gap),
    /// An op's signature did not verify, or its device is unrecognised. Checked before anything
    /// else in `on_ops`, so a bad batch never advances `wanted`/`inflight`.
    Crypto(CryptoError),
    /// A link-level `Hello`/workspace `Greet` was attempted or accepted before its precondition:
    /// `Session::on_link_hello` before our own `link_hello` was sent, or a workspace's `hello`/
    /// `on_hello` before the link handshake completed (`peer` known) — stage 2's "the link owns
    /// its own handshake state" guard (`session.rs` module doc).
    LinkNotReady,
    /// The link-level `Hello` was sent, or a peer `Hello` accepted, more than once on this
    /// session — refused, not silently reapplied, the same "no message twice" discipline
    /// `Unexpected` already gives every per-workspace state.
    LinkAlreadyGreeted,
    /// A link-level `on_link_hello` call was given a message that was not a `Hello` at all;
    /// names what arrived.
    NotAHello(&'static str),
}

impl SessionError {
    /// Stable snake_case event tag, one per variant — for structured logs, never the full
    /// `Display` sentence (task `logging-sync-crate`). `Crypto` collapses to the flat `"crypto"`
    /// tag here; the wrapped `CryptoError`'s own, more specific `kind()` is logged directly at the
    /// crypto call site that produced it instead of re-derived through this wrapper.
    pub(crate) fn kind(&self) -> &'static str {
        match self {
            SessionError::Unexpected { .. } => "unexpected",
            SessionError::UnknownWorkspace(_) => "unknown_workspace",
            SessionError::TooManyWorkspaces { .. } => "too_many_workspaces",
            SessionError::WorkspaceMismatch { .. } => "workspace_mismatch",
            SessionError::GroupMismatch { .. } => "group_mismatch",
            SessionError::ProtocolMismatch { .. } => "protocol_mismatch",
            SessionError::PeerAhead { .. } => "peer_ahead",
            SessionError::Unrequested(_) => "unrequested",
            SessionError::NotInBatch(_) => "not_in_batch",
            SessionError::Gap(_) => "gap",
            SessionError::Crypto(_) => "crypto",
            SessionError::LinkNotReady => "link_not_ready",
            SessionError::LinkAlreadyGreeted => "link_already_greeted",
            SessionError::NotAHello(_) => "not_a_hello",
        }
    }
}

impl fmt::Display for SessionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SessionError::Unexpected { state, what } => {
                write!(f, "{what} is not expected while {state:?}")
            }
            SessionError::UnknownWorkspace(id) => {
                write!(f, "workspace {id} is not open on this session")
            }
            SessionError::TooManyWorkspaces { len, max } => {
                write!(
                    f,
                    "opening this workspace would hold {len} open, over the cap of {max}"
                )
            }
            SessionError::WorkspaceMismatch { called, message } => write!(
                f,
                "message is for workspace {message} but {called} was asked for"
            ),
            SessionError::GroupMismatch { ours, theirs } => {
                write!(f, "peer group {theirs:?} is not our group {ours:?}")
            }
            SessionError::ProtocolMismatch { ours, theirs } => {
                write!(f, "peer protocol {theirs} is not our protocol {ours}")
            }
            SessionError::PeerAhead {
                peer_ms,
                local_ms,
                lead_ms,
            } => write!(
                f,
                "peer clock {peer_ms} ms leads local clock {local_ms} ms by {lead_ms} ms; refused"
            ),
            SessionError::Unrequested(r) => {
                write!(
                    f,
                    "ops for {:?} {}..={} were not wanted",
                    r.device, r.first, r.last
                )
            }
            SessionError::NotInBatch(r) => {
                write!(
                    f,
                    "committed {:?} {}..={} was not in the batch",
                    r.device, r.first, r.last
                )
            }
            SessionError::Gap(g) => write!(f, "{g}"),
            SessionError::Crypto(e) => write!(f, "{e}"),
            SessionError::LinkNotReady => {
                write!(f, "the link handshake is not ready for this yet")
            }
            SessionError::LinkAlreadyGreeted => {
                write!(f, "the link-level Hello was already sent or received")
            }
            SessionError::NotAHello(what) => write!(f, "{what} is not a Hello"),
        }
    }
}

impl std::error::Error for SessionError {}
