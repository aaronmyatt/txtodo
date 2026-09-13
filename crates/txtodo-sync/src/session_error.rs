//! Why a `Session` refused a message or a caller action. Split from `session.rs` for the file
//! budget; every variant names what was seen and what was expected (CLAUDE.md §3).

use std::fmt;

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
}

impl fmt::Display for SessionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SessionError::Unexpected { state, what } => {
                write!(f, "{what} is not expected while {state:?}")
            }
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
        }
    }
}

impl std::error::Error for SessionError {}
