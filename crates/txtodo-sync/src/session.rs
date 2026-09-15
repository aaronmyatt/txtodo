//! `Session`: a container of one sub-session per open workspace, multiplexing several workspaces'
//! Want/Ack bookkeeping over one `(device, group)` peer relationship (task
//! `daemon-workspace-session-multiplex`, root todo, stage 1 — split off `daemon-shared-sync-link`,
//! whose own notes deferred exactly this and made a binding design decision this task honors:
//! `Op`'s wire shape stays frozen, so the workspace dimension lives on `Message::Want`/`Ops`/`Ack`
//! only, never on `Op` itself).
//!
//! **One `Hello`, per-workspace sub-sessions.** A link negotiates device+group exactly once
//! (`GroupId` stays one shared id per device-set per ADR 0021, not per-workspace) — but at this
//! library's level, `hello`/`on_hello`/`on_ops`/`committed` are all keyed by an explicit
//! `WorkspaceId` parameter, so a caller (the read/write loop `daemon-workspace-session-multiplex`
//! stage 2 will build) can drive several workspaces' handshakes over the same `Session`,
//! interleaved in any order, without their bookkeeping crossing. Each workspace's own state
//! machine (`Idle → Greeted → Wanting → Importing → (Wanting | Idle)`) is unchanged from the
//! pre-multiplex design — see `workspace_session.rs`, where it now lives. This `Session` owns only
//! what is genuinely shared across every workspace on one link: this device's own id, the group,
//! and the peer's device id once learned from its `Hello`.
//!
//! Every transition matches state exhaustively with no default arm, so a new state cannot be
//! silently ignored. The skew guard runs on `Hello` (`txtodo_model::Skew`), before a single op is
//! accepted, and `Ack` carries only runs the caller reports as *committed* — never merely
//! received — so a crash mid-import re-requests them. Naming an unopened or unknown workspace is a
//! typed error (`SessionError::UnknownWorkspace`), never a panic — exactly as routine as any other
//! malformed caller input this crate refuses rather than asserts.

use std::collections::BTreeMap;

use txtodo_model::{DeviceId, Op, Skew};
use txtodo_store::WorkspaceId;

use crate::message::{GroupId, Heads, Message, OriginRange};
use crate::session_error::SessionError;
use crate::sign::DevicePublicKey;
use crate::workspace_session::WorkspaceSession;

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

/// The outcome of a valid peer `Hello`, for one workspace.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Greeting {
    /// The `Want` to send back (possibly empty: in sync).
    pub want: Message,
    /// How the peer's clock compared; `Behind` is safe and worth a warning.
    pub skew: Skew,
}

/// One peer relationship, multiplexing every open workspace's own sub-session.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Session {
    device: DeviceId,
    group: GroupId,
    /// The peer's device id, learned from its `Hello` — shared across every workspace on this
    /// link, set at most once per real handshake (a later `on_hello` call just re-confirms it).
    peer: Option<DeviceId>,
    workspaces: BTreeMap<WorkspaceId, WorkspaceSession>,
}

impl Session {
    /// A fresh session for this device and group, with no workspace open yet.
    pub fn new(device: DeviceId, group: GroupId) -> Session {
        Session {
            device,
            group,
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

    /// `workspace`'s `Idle → Greeted`: the `Hello` to send. `Hello` itself carries no workspace
    /// (module doc) — `workspace` only selects which sub-session advances.
    pub fn hello(&mut self, workspace: WorkspaceId, now_ms: u64) -> Result<Message, SessionError> {
        let (device, group) = (self.device, self.group);
        self.workspace_mut(workspace)?.hello(device, group, now_ms)
    }

    /// `workspace`'s `Greeted → Wanting` (or `Idle`): checks group, protocol and clock skew, then
    /// derives that workspace's own `Want`. Records the peer's device id (shared across every
    /// workspace on this link) once learned.
    pub fn on_hello(
        &mut self,
        workspace: WorkspaceId,
        msg: &Message,
        now_ms: u64,
    ) -> Result<Greeting, SessionError> {
        let group = self.group;
        let (greeting, peer) = self
            .workspace_mut(workspace)?
            .on_hello(group, msg, now_ms, workspace)?;
        self.peer = Some(peer);
        Ok(greeting)
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
