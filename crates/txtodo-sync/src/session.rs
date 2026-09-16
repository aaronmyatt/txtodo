//! `Session`: a container of one sub-session per open workspace, multiplexing several workspaces'
//! Want/Ack bookkeeping over one `(device, group)` peer relationship (task
//! `daemon-workspace-session-multiplex`, root todo, stage 1 — split off `daemon-shared-sync-link`,
//! whose own notes deferred exactly this and made a binding design decision this task honors:
//! `Op`'s wire shape stays frozen, so the workspace dimension lives on `Message::Want`/`Ops`/`Ack`
//! only, never on `Op` itself).
//!
//! **One `Hello`, per-workspace `Greet`s.** A link negotiates device+group exactly once
//! (`GroupId` stays one shared id per device-set per ADR 0021, not per-workspace) via
//! [`Session::link_hello`]/[`Session::on_link_hello`] — sent/consumed exactly once per connection,
//! before any workspace's own `Greet` is legal. At this library's level, the per-workspace
//! `hello`/`on_hello`/`on_ops`/`committed` are all keyed by an explicit `WorkspaceId` parameter, so
//! a caller (`txtodo-daemon`'s `lan_session.rs::drive_shared_session`, stage 2) can drive several
//! workspaces' `Greet`/`Want`/`Ops`/`Ack` exchanges over the same `Session`, interleaved in any
//! order, without their bookkeeping crossing. Each workspace's own state machine (`Idle → Greeted →
//! Wanting → Importing → (Wanting | Idle)`) is unchanged from the pre-multiplex design — see
//! `workspace_session.rs`, where it now lives, except that stage 2 moved the group/protocol/skew
//! checks that used to run on every workspace's own `Hello` up to `on_link_hello`, since they only
//! need to happen once per link. This `Session` owns only what is genuinely shared across every
//! workspace on one link: this device's own id, the group, whether our own link `Hello` was sent,
//! and the peer's device id once learned from its `Hello`.
//!
//! **Stage 2 design decision, recorded here since this is where it lives:** `Session` now owns
//! "have we sent our link-level identity yet" (`link_hello_sent`), not `txtodo-daemon`'s
//! `lan_session.rs` — stage 1 left this as an open question (`lan_session.rs::initial_hello` sat
//! entirely outside `Session`). Moving it in means a caller cannot accidentally send a workspace's
//! `Greet` before the link handshake, or accept one before the peer's `Hello` validated group/
//! protocol/skew (`on_hello`'s own `peer.is_none()` guard) — `Session` enforces the ordering itself
//! rather than trusting every caller to get it right, the same reasoning that already justified
//! `Session` owning the per-workspace state machines instead of leaving that to callers.
//!
//! Every transition matches state exhaustively with no default arm, so a new state cannot be
//! silently ignored. The skew guard runs on the link-level `Hello` (`txtodo_model::Skew`), before a
//! single op is accepted for any workspace, and `Ack` carries only runs the caller reports as
//! *committed* — never merely received — so a crash mid-import re-requests them. Naming an unopened
//! or unknown workspace is a typed error (`SessionError::UnknownWorkspace`), never a panic —
//! exactly as routine as any other malformed caller input this crate refuses rather than asserts.

use std::collections::BTreeMap;

use txtodo_model::{DeviceId, Op, Skew};
use txtodo_store::WorkspaceId;

use crate::message::{GroupId, Heads, Message, OriginRange};
use crate::session_error::SessionError;
use crate::sign::DevicePublicKey;
use crate::workspace_session::{self, WorkspaceSession};

/// Most workspaces one `Session` multiplexes at once — a device realistically opens far fewer
/// than this; the cap exists so nothing here can grow without limit (every collection in this
/// crate has a named, checked cap), mirroring `txtodo-daemon`'s own `MAX_ROUTED_WORKSPACES`.
pub const MAX_OPEN_WORKSPACES: usize = 256;

/// Where one workspace's sub-session is. Closed set.
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

/// One peer relationship, multiplexing every open workspace's own sub-session.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Session {
    device: DeviceId,
    group: GroupId,
    /// Whether our own link-level `Hello` was already sent (`link_hello`) — a second call, or a
    /// peer `Hello` arriving before this is true, is refused (`SessionError::LinkNotReady`/
    /// `LinkAlreadyGreeted`).
    link_hello_sent: bool,
    /// The peer's device id, learned from its `Hello` — shared across every workspace on this
    /// link, set at most once per real handshake (`on_link_hello` refuses a second one).
    peer: Option<DeviceId>,
    workspaces: BTreeMap<WorkspaceId, WorkspaceSession>,
}

impl Session {
    /// A fresh session for this device and group, with no workspace open yet and its link-level
    /// handshake not started.
    pub fn new(device: DeviceId, group: GroupId) -> Session {
        Session {
            device,
            group,
            link_hello_sent: false,
            peer: None,
            workspaces: BTreeMap::new(),
        }
    }

    /// This device's own id.
    pub fn device(&self) -> DeviceId {
        self.device
    }

    /// The shared sync group.
    pub fn group(&self) -> GroupId {
        self.group
    }

    /// The peer's device id, once learned from its `Hello`.
    pub fn peer(&self) -> Option<DeviceId> {
        self.peer
    }

    /// Whether `workspace` is currently open on this session.
    pub fn is_open(&self, workspace: WorkspaceId) -> bool {
        self.workspaces.contains_key(&workspace)
    }

    /// Opens `workspace` in a fresh `Idle` sub-session seeded with `heads` — what this device
    /// already holds for it. Idempotent-in-place for an id already open (a workspace reopened, or
    /// a redundant open call): only a genuinely new id counts against `MAX_OPEN_WORKSPACES`, and
    /// re-opening one discards whatever sub-session state it held (a caller that wants to keep an
    /// open workspace's bookkeeping should not call this again for it).
    pub fn open_workspace(
        &mut self,
        workspace: WorkspaceId,
        heads: Heads,
    ) -> Result<(), SessionError> {
        if !self.workspaces.contains_key(&workspace) && self.workspaces.len() >= MAX_OPEN_WORKSPACES
        {
            return Err(SessionError::TooManyWorkspaces {
                len: self.workspaces.len() + 1,
                max: MAX_OPEN_WORKSPACES,
            });
        }
        self.workspaces
            .insert(workspace, WorkspaceSession::new(heads));
        debug_assert!(self.workspaces.len() <= MAX_OPEN_WORKSPACES);
        Ok(())
    }

    /// `workspace`'s current state.
    pub fn state(&self, workspace: WorkspaceId) -> Result<SessionState, SessionError> {
        Ok(self.workspace(workspace)?.state())
    }

    /// `workspace`'s own heads, advanced as its runs commit.
    pub fn heads(&self, workspace: WorkspaceId) -> Result<&Heads, SessionError> {
        Ok(self.workspace(workspace)?.heads())
    }

    /// `workspace`'s runs still outstanding.
    pub fn wanted(&self, workspace: WorkspaceId) -> Result<&[OriginRange], SessionError> {
        Ok(self.workspace(workspace)?.wanted())
    }

    /// Our own link-level `Hello`: device, group, protocol and wall clock, sent exactly once per
    /// connection before any workspace's own `Greet` is legal to send. `heads` is always empty —
    /// `Hello`'s field layout is frozen (module doc); each workspace reports its own heads via
    /// `Greet` instead. A second call is `SessionError::LinkAlreadyGreeted`. A thin span wrapper
    /// around `link_hello_inner` (`#[instrument]` on the real body overflows the cognitive-
    /// complexity budget, `tasks/logging-sync-crate/notes.md`).
    #[tracing::instrument(skip_all, fields(device = %self.device, group = ?self.group))]
    pub fn link_hello(&mut self, now_ms: u64) -> Result<Message, SessionError> {
        let r = self.link_hello_inner(now_ms);
        log_link_hello(&r);
        r
    }

    fn link_hello_inner(&mut self, now_ms: u64) -> Result<Message, SessionError> {
        if self.link_hello_sent {
            return Err(SessionError::LinkAlreadyGreeted);
        }
        self.link_hello_sent = true;
        Ok(Message::Hello {
            device: self.device,
            group: self.group,
            heads: Heads::new(),
            protocol: crate::frame::PROTOCOL_VERSION,
            wall_ms: now_ms,
        })
    }

    /// Consumes the peer's link-level `Hello`: checks group, protocol and clock skew, and records
    /// the peer's device id. Requires our own `link_hello` to have been sent first
    /// (`SessionError::LinkNotReady` otherwise) and refuses a second peer `Hello`
    /// (`SessionError::LinkAlreadyGreeted`) — the link-level counterpart of the per-workspace state
    /// machine's own "no message twice" discipline. Every open (or later-opened) workspace's own
    /// `Greet`/`on_hello` requires this to have succeeded first (`peer` known). Wrapper/inner split,
    /// same reason as `link_hello`.
    #[tracing::instrument(skip_all, fields(device = %self.device, group = ?self.group))]
    pub fn on_link_hello(&mut self, msg: &Message, now_ms: u64) -> Result<Skew, SessionError> {
        let r = self.on_link_hello_inner(msg, now_ms);
        log_on_link_hello(&r);
        r
    }

    fn on_link_hello_inner(&mut self, msg: &Message, now_ms: u64) -> Result<Skew, SessionError> {
        if !self.link_hello_sent {
            return Err(SessionError::LinkNotReady);
        }
        if self.peer.is_some() {
            return Err(SessionError::LinkAlreadyGreeted);
        }
        let Message::Hello {
            device,
            group: theirs,
            protocol,
            wall_ms,
            ..
        } = msg
        else {
            return Err(SessionError::NotAHello(workspace_session::name_of(msg)));
        };
        if *theirs != self.group {
            return Err(SessionError::GroupMismatch {
                ours: self.group,
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
        self.peer = Some(*device);
        Ok(skew)
    }

    /// `workspace`'s `Idle → Greeted`: the `Greet` to send (device/group/protocol/skew were
    /// already handled once by `link_hello`/`on_link_hello`).
    pub fn hello(&mut self, workspace: WorkspaceId) -> Result<Message, SessionError> {
        self.workspace_mut(workspace)?.hello(workspace)
    }

    /// `workspace`'s `Greeted → Wanting` (or `Idle`): consumes the peer's `Greet` for this
    /// workspace and derives our own `Want`. Requires the link handshake to already be done
    /// (`peer` known) — a workspace cannot be greeted before `on_link_hello` succeeded
    /// (`SessionError::LinkNotReady`), so a hostile or buggy peer can never skip the
    /// group/protocol/skew checks by going straight for a workspace's `Greet`. Checked *after*
    /// confirming `workspace` is actually open: an unopened workspace is always
    /// `SessionError::UnknownWorkspace`, the more specific and actionable error, never masked by
    /// `LinkNotReady` just because the link handshake also happens not to be done yet.
    pub fn on_hello(
        &mut self,
        workspace: WorkspaceId,
        msg: &Message,
    ) -> Result<Message, SessionError> {
        if !self.is_open(workspace) {
            return Err(SessionError::UnknownWorkspace(workspace));
        }
        if self.peer.is_none() {
            return Err(SessionError::LinkNotReady);
        }
        self.workspace_mut(workspace)?.on_hello(msg, workspace)
    }

    /// `workspace`'s `Wanting → Importing`. `msg` must be a `Message::Ops` whose own `workspace`
    /// field matches `workspace` (`SessionError::WorkspaceMismatch` otherwise) and must already be
    /// opened (see `sealed_ops::open_ops`) — `Session` never touches the group-key AEAD, only
    /// per-op signatures via `device_keys`.
    pub fn on_ops(
        &mut self,
        workspace: WorkspaceId,
        msg: &Message,
        device_keys: &BTreeMap<DeviceId, DevicePublicKey>,
    ) -> Result<Vec<Op>, SessionError> {
        self.workspace_mut(workspace)?
            .on_ops(workspace, msg, device_keys)
    }

    /// `workspace`'s `Importing → Wanting | Idle`: the caller reports what it durably committed;
    /// that workspace's heads advance and the `Ack` to send carries exactly those runs.
    pub fn committed(
        &mut self,
        workspace: WorkspaceId,
        ranges: &[OriginRange],
    ) -> Result<Message, SessionError> {
        self.workspace_mut(workspace)?.committed(workspace, ranges)
    }

    fn workspace(&self, workspace: WorkspaceId) -> Result<&WorkspaceSession, SessionError> {
        self.workspaces
            .get(&workspace)
            .ok_or(SessionError::UnknownWorkspace(workspace))
    }

    fn workspace_mut(
        &mut self,
        workspace: WorkspaceId,
    ) -> Result<&mut WorkspaceSession, SessionError> {
        self.workspaces
            .get_mut(&workspace)
            .ok_or(SessionError::UnknownWorkspace(workspace))
    }
}

/// Split out so the event macro doesn't count against `link_hello`'s own `#[instrument]` budget.
/// One unconditional event, `kind` present only on `Err` — a `match`'s own branches inside a
/// `tracing` macro call cost real `cognitive_complexity` points on their own
/// (`tasks/logging-sync-crate/notes.md`), so this stays branch-free and lets `Option<&str>`'s own
/// `tracing::Value` impl (empty field when `None`) do the conditional part instead.
fn log_link_hello(r: &Result<Message, SessionError>) {
    tracing::debug!(
        ok = r.is_ok(),
        kind = r.as_ref().err().map(SessionError::kind),
        "link_hello"
    );
}

/// Split out for the same reason as `log_link_hello`.
fn log_on_link_hello(r: &Result<Skew, SessionError>) {
    tracing::debug!(
        ok = r.is_ok(),
        kind = r.as_ref().err().map(SessionError::kind),
        "link_hello_received"
    );
}
