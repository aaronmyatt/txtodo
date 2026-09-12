//! One receiving session: `Idle → Greeted → Wanting → Importing → (Wanting | Idle)`. Transport-
//! agnostic and store-agnostic — the caller moves frames and commits ops; the session only decides
//! what is legal next and what to send. Every transition matches the state exhaustively with no
//! default arm, so a new state cannot be silently ignored. The skew guard runs on `Hello`
//! (`txtodo_model::Skew`), before a single op is accepted, and `Ack` carries only runs the caller
//! reports as *committed* — never merely received — so a crash mid-import re-requests them.

use txtodo_model::{DeviceId, Op, Skew};

use crate::frame::PROTOCOL_VERSION;
use crate::message::{GroupId, Heads, Message, OriginRange};
use crate::session_error::SessionError;
use crate::want::{advance, want};

/// Where the session is. Closed set.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionState {
    /// Nothing sent yet.
    Idle,
    /// Our `Hello` is out; waiting for theirs.
    Greeted,
    /// Our `Want` is out; waiting for `Ops`.
    Wanting,
    /// A batch is with the caller to commit; waiting for `committed()`.
    Importing,
}

/// The outcome of a valid peer `Hello`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Greeting {
    /// The `Want` to send back (possibly empty: in sync).
    pub want: Message,
    /// How the peer's clock compared; `Behind` is safe and worth a warning.
    pub skew: Skew,
}

/// The receiving half of one sync with one peer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Session {
    state: SessionState,
    device: DeviceId,
    group: GroupId,
    heads: Heads,
    peer: Option<DeviceId>,
    /// Runs still to receive, in device order.
    wanted: Vec<OriginRange>,
    /// Runs in the batch the caller is committing.
    inflight: Vec<OriginRange>,
}

impl Session {
    /// A fresh `Idle` session for this device, group and what we already hold.
    pub fn new(device: DeviceId, group: GroupId, heads: Heads) -> Session {
        Session {
            state: SessionState::Idle,
            device,
            group,
            heads,
            peer: None,
            wanted: Vec::new(),
            inflight: Vec::new(),
        }
    }

    /// Current state.
    pub fn state(&self) -> SessionState {
        self.state
    }

    /// Our heads, advanced as runs commit.
    pub fn heads(&self) -> &Heads {
        &self.heads
    }

    /// Runs still outstanding.
    pub fn wanted(&self) -> &[OriginRange] {
        &self.wanted
    }

    /// `Idle → Greeted`: the `Hello` to send.
    pub fn hello(&mut self, now_ms: u64) -> Result<Message, SessionError> {
        match self.state {
            SessionState::Idle => {}
            SessionState::Greeted | SessionState::Wanting | SessionState::Importing => {
                return Err(self.unexpected("hello()"));
            }
        }
        self.state = SessionState::Greeted;
        debug_assert!(self.peer.is_none() && self.wanted.is_empty());
        Ok(Message::Hello {
            device: self.device,
            group: self.group,
            heads: self.heads.clone(),
            protocol: PROTOCOL_VERSION,
            wall_ms: now_ms,
        })
    }

    /// `Greeted → Wanting` (or `Idle` when nothing is wanted): checks group, protocol and clock
    /// skew, then derives the `Want`.
    pub fn on_hello(&mut self, msg: &Message, now_ms: u64) -> Result<Greeting, SessionError> {
        match self.state {
            SessionState::Greeted => {}
            SessionState::Idle | SessionState::Wanting | SessionState::Importing => {
                return Err(self.unexpected("Hello"));
            }
        }
        let Message::Hello {
            device,
            group,
            heads,
            protocol,
            wall_ms,
        } = msg
        else {
            return Err(self.unexpected(name_of(msg)));
        };
        if *group != self.group {
            return Err(SessionError::GroupMismatch {
                ours: self.group,
                theirs: *group,
            });
        }
        if *protocol != PROTOCOL_VERSION {
            return Err(SessionError::ProtocolMismatch {
                ours: PROTOCOL_VERSION,
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
        self.peer = Some(*device);
        self.wanted = want(&self.heads, heads);
        self.state = if self.wanted.is_empty() {
            SessionState::Idle
        } else {
            SessionState::Wanting
        };
        debug_assert!(self.inflight.is_empty());
        debug_assert!(!matches!(skew, Skew::Ahead(_)));
        Ok(Greeting {
            want: Message::Want {
                ranges: self.wanted.clone(),
            },
            skew,
        })
    }

    /// `Wanting → Importing`: hands the batch to the caller to commit. Every run in the batch
    /// must lie inside a run we asked for.
    pub fn on_ops(&mut self, msg: &Message) -> Result<Vec<Op>, SessionError> {
        match self.state {
            SessionState::Wanting => {}
            SessionState::Idle | SessionState::Greeted | SessionState::Importing => {
                return Err(self.unexpected("Ops"));
            }
        }
        let Message::Ops { ops, ranges } = msg else {
            return Err(self.unexpected(name_of(msg)));
        };
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
    pub fn committed(&mut self, ranges: &[OriginRange]) -> Result<Message, SessionError> {
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
