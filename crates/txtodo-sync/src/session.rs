//! One receiving session: `Idle → Greeted → Wanting → Importing → (Wanting | Idle)`. Transport-
//! agnostic and store-agnostic — the caller moves frames and commits ops; the session only decides
//! what is legal next and what to send. Every transition matches the state exhaustively with no
//! default arm, so a new state cannot be silently ignored. The skew guard runs on `Hello`
//! (`txtodo_model::Skew`), before a single op is accepted, and `Ack` carries only runs the caller
//! reports as *committed* — never merely received — so a crash mid-import re-requests them.

use txtodo_model::{DeviceId, Skew};

use crate::frame::PROTOCOL_VERSION;
use crate::message::{GroupId, Heads, Message, OriginRange};
use crate::session_error::SessionError;
use crate::want::want;

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

    fn unexpected(&self, what: &'static str) -> SessionError {
        SessionError::Unexpected {
            state: self.state,
            what,
        }
    }
}

fn name_of(msg: &Message) -> &'static str {
    match msg {
        Message::Hello { .. } => "Hello",
        Message::Want { .. } => "Want",
        Message::Ops { .. } => "Ops",
        Message::Ack { .. } => "Ack",
    }
}
