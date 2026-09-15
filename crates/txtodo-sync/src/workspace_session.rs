//! One workspace's own sub-session state machine: `Idle → Greeted → Wanting → Importing →
//! (Wanting | Idle)`, unchanged in spirit from the pre-multiplex, one-workspace-per-`Session`
//! design (`session.rs`'s own doc names why it moved here). [`Session`](crate::session::Session)
//! owns the device/group/peer facts shared by every workspace on one link; a `WorkspaceSession`
//! owns only what genuinely differs per workspace — its own state, heads, wanted and inflight
//! runs — so two workspaces' bookkeeping can never cross-contaminate even when their `hello`/
//! `on_hello`/`on_ops`/`committed` calls interleave on the same `Session`.

use std::collections::BTreeMap;

use txtodo_model::{DeviceId, Op, Skew, Ulid};
use txtodo_store::WorkspaceId;

use crate::message::{GroupId, Heads, Message, OriginRange};
use crate::session::{Greeting, SessionState};
use crate::session_error::SessionError;
use crate::sign::{DevicePublicKey, verify_batch};
use crate::want::{advance, want};

/// One workspace's own piece of a multiplexed `Session`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct WorkspaceSession {
    state: SessionState,
    heads: Heads,
    /// Runs still to receive, in device order.
    wanted: Vec<OriginRange>,
    /// Runs in the batch the caller is committing.
    inflight: Vec<OriginRange>,
}

impl WorkspaceSession {
    /// A fresh `Idle` sub-session seeded with what this workspace already holds.
    pub(crate) fn new(heads: Heads) -> WorkspaceSession {
        WorkspaceSession {
            state: SessionState::Idle,
            heads,
            wanted: Vec::new(),
            inflight: Vec::new(),
        }
    }

    pub(crate) fn state(&self) -> SessionState {
        self.state
    }

    pub(crate) fn heads(&self) -> &Heads {
        &self.heads
    }

    pub(crate) fn wanted(&self) -> &[OriginRange] {
        &self.wanted
    }

    /// `Idle → Greeted`: the `Hello` to send. `device`/`group` are the link-level facts
    /// `Session` shares across every workspace — `Hello` itself carries no workspace (module doc,
    /// `message.rs`).
    pub(crate) fn hello(
        &mut self,
        device: DeviceId,
        group: GroupId,
        now_ms: u64,
    ) -> Result<Message, SessionError> {
        match self.state {
            SessionState::Idle => {}
            SessionState::Greeted | SessionState::Wanting | SessionState::Importing => {
                return Err(self.unexpected("hello()"));
            }
        }
        self.state = SessionState::Greeted;
        debug_assert!(self.wanted.is_empty());
        Ok(Message::Hello {
            device,
            group,
            heads: self.heads.clone(),
            protocol: crate::frame::PROTOCOL_VERSION,
            wall_ms: now_ms,
        })
    }

    /// `Greeted → Wanting` (or `Idle` when nothing is wanted): checks group, protocol and clock
    /// skew against the link-level facts `Session` passes in, then derives this workspace's own
    /// `Want`. Returns the peer's device id so `Session` can record it (shared across every
    /// workspace on this link).
    pub(crate) fn on_hello(
        &mut self,
        group: GroupId,
        msg: &Message,
        now_ms: u64,
        workspace: WorkspaceId,
    ) -> Result<(Greeting, DeviceId), SessionError> {
        match self.state {
            SessionState::Greeted => {}
            SessionState::Idle | SessionState::Wanting | SessionState::Importing => {
                return Err(self.unexpected("Hello"));
            }
        }
        let (device, skew) = self.check_hello(group, msg, now_ms)?;
        self.wanted = want(&self.heads, hello_heads(msg));
        self.state = if self.wanted.is_empty() {
            SessionState::Idle
        } else {
            SessionState::Wanting
        };
        debug_assert!(self.inflight.is_empty());
        debug_assert!(!matches!(skew, Skew::Ahead(_)));
        let want = Message::Want {
            workspace: workspace.ulid().to_u128(),
            ranges: self.wanted.clone(),
        };
        Ok((Greeting { want, skew }, device))
    }

    /// The group/protocol/skew checks `on_hello` runs before touching any workspace state — split
    /// out purely to keep `on_hello` under this workspace's cyclomatic-complexity budget.
    fn check_hello(
        &self,
        group: GroupId,
        msg: &Message,
        now_ms: u64,
    ) -> Result<(DeviceId, Skew), SessionError> {
        let Message::Hello {
            device,
            group: theirs,
            protocol,
            wall_ms,
            ..
        } = msg
        else {
            return Err(self.unexpected(name_of(msg)));
        };
        if *theirs != group {
            return Err(SessionError::GroupMismatch {
                ours: group,
                theirs: *theirs,
            });
        }
        if *protocol != crate::frame::PROTOCOL_VERSION {
            return Err(SessionError::ProtocolMismatch {
                ours: crate::frame::PROTOCOL_VERSION,
                theirs: *protocol,
            });
        }
        let skew = Skew::check(*wall_ms, now_ms);
        if let Skew::Ahead(lead_ms) = skew {
            return Err(SessionError::PeerAhead {
                peer_ms: *wall_ms,
                local_ms: now_ms,
                lead_ms,
            });
        }
        Ok((*device, skew))
    }

    /// `Wanting → Importing`: hands the batch to the caller to commit. `msg`'s own `workspace`
    /// field must match `workspace` — a caller routing to the wrong sub-session is a typed error,
    /// never a silent misroute. Every op's signature is verified against `device_keys` before
    /// anything else runs (`sign::verify_batch` is all-or-nothing); only once authorship checks
    /// out does a run outside our `Want` get checked. `msg` must already be opened (see
    /// `sealed_ops::open_ops`) — this never touches the group-key AEAD, only per-op signatures.
    pub(crate) fn on_ops(
        &mut self,
        workspace: WorkspaceId,
        msg: &Message,
        device_keys: &BTreeMap<DeviceId, DevicePublicKey>,
    ) -> Result<Vec<Op>, SessionError> {
        match self.state {
            SessionState::Wanting => {}
            SessionState::Idle | SessionState::Greeted | SessionState::Importing => {
                return Err(self.unexpected("Ops"));
            }
        }
        let Message::Ops {
            workspace: msg_ws,
            ops,
            signatures,
            ranges,
        } = msg
        else {
            return Err(self.unexpected(name_of(msg)));
        };
        check_workspace(workspace, *msg_ws)?;
        verify_batch(ops, signatures, device_keys).map_err(SessionError::Crypto)?;
        if let Some(stray) = ranges.iter().find(|r| !covered(&self.wanted, r)) {
            return Err(SessionError::Unrequested(*stray));
        }
        self.inflight = ranges.clone();
        self.state = SessionState::Importing;
        debug_assert!(self.inflight.iter().all(|r| covered(&self.wanted, r)));
        debug_assert_eq!(self.state, SessionState::Importing);
        Ok(ops.clone())
    }

    /// `Importing → Wanting | Idle`: the caller reports what it durably committed; heads advance
    /// and the `Ack` to send carries exactly those runs. A run outside the batch is refused.
    pub(crate) fn committed(
        &mut self,
        workspace: WorkspaceId,
        ranges: &[OriginRange],
    ) -> Result<Message, SessionError> {
        match self.state {
            SessionState::Importing => {}
            SessionState::Idle | SessionState::Greeted | SessionState::Wanting => {
                return Err(self.unexpected("committed()"));
            }
        }
        if let Some(stray) = ranges.iter().find(|r| !covered(&self.inflight, r)) {
            return Err(SessionError::NotInBatch(*stray));
        }
        let mut heads = self.heads.clone();
        for r in ranges {
            advance(&mut heads, r).map_err(SessionError::Gap)?;
        }
        self.heads = heads;
        consume(&mut self.wanted, ranges);
        self.inflight.clear();
        self.state = if self.wanted.is_empty() {
            SessionState::Idle
        } else {
            SessionState::Wanting
        };
        debug_assert!(self.wanted.iter().all(|r| r.first <= r.last));
        debug_assert!(self.inflight.is_empty());
        Ok(Message::Ack {
            workspace: workspace.ulid().to_u128(),
            committed: ranges.to_vec(),
        })
    }

    fn unexpected(&self, what: &'static str) -> SessionError {
        SessionError::Unexpected {
            state: self.state,
            what,
        }
    }
}

/// `msg`'s own `workspace` field must match `workspace` — a mismatch is refused before any other
/// check (the same "validated, never asserted" precedent `GroupMismatch`/`ProtocolMismatch`
/// already set for `on_hello`).
fn check_workspace(workspace: WorkspaceId, msg_ws: u128) -> Result<(), SessionError> {
    let msg_ws = WorkspaceId::new(Ulid::from_u128(msg_ws));
    if msg_ws != workspace {
        return Err(SessionError::WorkspaceMismatch {
            called: workspace,
            message: msg_ws,
        });
    }
    Ok(())
}

/// `msg` must be a `Hello` by the time this runs (`check_hello` already refused anything else).
fn hello_heads(msg: &Message) -> &Heads {
    match msg {
        Message::Hello { heads, .. } => heads,
        Message::Want { .. } | Message::Ops { .. } | Message::Ack { .. } => {
            unreachable!("check_hello already refused a non-Hello message")
        }
    }
}

/// True when `r` lies within one of `runs` (same device, inside its bounds).
fn covered(runs: &[OriginRange], r: &OriginRange) -> bool {
    runs.iter()
        .any(|w| w.device == r.device && w.first <= r.first && r.last <= w.last)
}

/// Drops the committed prefix of each wanted run; a run fully covered disappears.
fn consume(wanted: &mut Vec<OriginRange>, committed: &[OriginRange]) {
    let before = wanted.len();
    for c in committed {
        for w in wanted.iter_mut() {
            if w.device == c.device && c.last >= w.first {
                w.first = c.last + 1;
            }
        }
    }
    wanted.retain(|w| w.first <= w.last);
    debug_assert!(wanted.len() <= before, "consume never adds a run");
    debug_assert!(wanted.iter().all(|w| w.first <= w.last));
}

fn name_of(msg: &Message) -> &'static str {
    match msg {
        Message::Hello { .. } => "Hello",
        Message::Want { .. } => "Want",
        Message::Ops { .. } => "Ops",
        Message::Ack { .. } => "Ack",
    }
}
