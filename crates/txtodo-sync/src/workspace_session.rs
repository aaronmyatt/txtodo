//! One workspace's own sub-session state machine: `Idle → Greeted → Wanting → Importing →
//! (Wanting | Idle)`, unchanged in spirit from the pre-multiplex, one-workspace-per-`Session`
//! design (`session.rs`'s own doc names why it moved here). [`Session`](crate::session::Session)
//! owns the device/group/peer facts shared by every workspace on one link — including, since
//! stage 2, the link-level `Hello` handshake itself (`Session::link_hello`/`on_link_hello`) — a
//! `WorkspaceSession` owns only what genuinely differs per workspace — its own state, heads,
//! wanted and inflight runs — so two workspaces' bookkeeping can never cross-contaminate even when
//! their `hello`/`on_hello`/`on_ops`/`committed` calls interleave on the same `Session`.
//!
//! **Stage 2:** this workspace's own `hello`/`on_hello` now build/consume `Message::Greet`, not
//! `Message::Hello` — the group/protocol/skew checks `check_hello` used to run here moved up to
//! `Session::on_link_hello` (they only need to happen once per link, not once per workspace), so
//! `on_hello` here is now just "consume the peer's heads for this workspace, derive our `Want`".

use std::collections::BTreeMap;

use txtodo_model::{DeviceId, Op, Ulid};
use txtodo_store::WorkspaceId;

use crate::message::{Heads, Message, OriginRange};
use crate::session::SessionState;
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
    /// Whether the peer's `Greet` for this workspace was consumed. Once it was, an `Ops` batch the
    /// peer pushes unasked is accepted in `Idle` (task `sync-live-push`, decided 2026-09-23): the
    /// peer already proved it holds the group key, and push is how a commit reaches it live.
    greeted: bool,
}

impl WorkspaceSession {
    /// A fresh `Idle` sub-session seeded with what this workspace already holds.
    pub(crate) fn new(heads: Heads) -> WorkspaceSession {
        WorkspaceSession {
            state: SessionState::Idle,
            heads,
            wanted: Vec::new(),
            inflight: Vec::new(),
            greeted: false,
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

    /// `Idle → Greeted`: the `Greet` to send. Stage 2: this used to be the `Hello` itself
    /// (single-workspace design); now the link-level `Hello` is `Session::link_hello`'s own job,
    /// sent once per connection, and this is purely this workspace's own announcement of what it
    /// already holds. A thin span wrapper around `hello_inner` (`#[instrument]` on the real body
    /// overflows the cognitive-complexity budget, `tasks/logging-sync-crate/notes.md`).
    #[tracing::instrument(skip_all, fields(from = ?self.state))]
    pub(crate) fn hello(&mut self, workspace: WorkspaceId) -> Result<Message, SessionError> {
        let r = self.hello_inner(workspace);
        log_state_result("workspace_hello", &r, self.state);
        r
    }

    fn hello_inner(&mut self, workspace: WorkspaceId) -> Result<Message, SessionError> {
        match self.state {
            SessionState::Idle => {}
            SessionState::Greeted | SessionState::Wanting | SessionState::Importing => {
                return Err(self.unexpected("hello()"));
            }
        }
        self.state = SessionState::Greeted;
        debug_assert!(self.wanted.is_empty());
        Ok(Message::Greet {
            workspace: workspace.ulid().to_u128(),
            heads: self.heads.clone(),
        })
    }

    /// `Greeted → Wanting` (or `Idle` when nothing is wanted): consumes the peer's `Greet` for this
    /// workspace and derives our own `Want`. Stage 2: no group/protocol/skew check here any more —
    /// `Session::on_link_hello` already ran those once, before any workspace's `Greet` is legal to
    /// send or accept (`Session::on_hello`'s own `peer.is_none()` guard). Wrapper/inner split, same
    /// reason as `hello`.
    #[tracing::instrument(skip_all, fields(from = ?self.state))]
    pub(crate) fn on_hello(
        &mut self,
        msg: &Message,
        workspace: WorkspaceId,
    ) -> Result<Message, SessionError> {
        let r = self.on_hello_inner(msg, workspace);
        log_state_result("workspace_greet_received", &r, self.state);
        r
    }

    fn on_hello_inner(
        &mut self,
        msg: &Message,
        workspace: WorkspaceId,
    ) -> Result<Message, SessionError> {
        match self.state {
            SessionState::Greeted => {}
            SessionState::Idle | SessionState::Wanting | SessionState::Importing => {
                return Err(self.unexpected("Greet"));
            }
        }
        let Message::Greet {
            workspace: msg_ws,
            heads: their_heads,
        } = msg
        else {
            return Err(self.unexpected(name_of(msg)));
        };
        check_workspace(workspace, *msg_ws)?;
        self.greeted = true;
        self.wanted = want(&self.heads, their_heads);
        self.state = if self.wanted.is_empty() {
            SessionState::Idle
        } else {
            SessionState::Wanting
        };
        debug_assert!(self.inflight.is_empty());
        Ok(Message::Want {
            workspace: workspace.ulid().to_u128(),
            ranges: self.wanted.clone(),
        })
    }

    /// `Wanting → Importing`, or `Idle → Importing` for a batch the peer pushed unasked once its
    /// `Greet` was consumed (task `sync-live-push`): hands the batch to the caller to commit.
    /// `msg`'s own `workspace` field must match `workspace` — a caller routing to the wrong
    /// sub-session is a typed error,
    /// never a silent misroute. Every op's signature is verified against `device_keys` before
    /// anything else runs (`sign::verify_batch` is all-or-nothing); only once authorship checks
    /// out does a run outside our `Want` get checked. `msg` must already be opened (see
    /// `sealed_ops::open_ops`) — this never touches the group-key AEAD, only per-op signatures.
    /// Wrapper/inner split, same reason as `hello`.
    #[tracing::instrument(skip_all, fields(from = ?self.state))]
    pub(crate) fn on_ops(
        &mut self,
        workspace: WorkspaceId,
        msg: &Message,
        device_keys: &BTreeMap<DeviceId, DevicePublicKey>,
    ) -> Result<Vec<Op>, SessionError> {
        let r = self.on_ops_inner(workspace, msg, device_keys);
        log_ops_result(&r);
        r
    }

    fn on_ops_inner(
        &mut self,
        workspace: WorkspaceId,
        msg: &Message,
        device_keys: &BTreeMap<DeviceId, DevicePublicKey>,
    ) -> Result<Vec<Op>, SessionError> {
        let pushed = match self.state {
            SessionState::Wanting => false,
            SessionState::Idle if self.greeted => true,
            SessionState::Idle | SessionState::Greeted | SessionState::Importing => {
                return Err(self.unexpected("Ops"));
            }
        };
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
        self.check_ranges(pushed, ranges)?;
        self.inflight = ranges.clone();
        self.state = SessionState::Importing;
        debug_assert_eq!(self.state, SessionState::Importing);
        Ok(ops.clone())
    }

    /// A wanted batch must lie inside our `Want`. A pushed one must follow the heads we hold with
    /// no gap and no repeat, run after run, so `committed` can always advance past it: a push that
    /// raced an exchange still in flight is refused here, and the next session's `Greet` fills in.
    fn check_ranges(&self, pushed: bool, ranges: &[OriginRange]) -> Result<(), SessionError> {
        if pushed {
            let mut trial = self.heads.clone();
            for r in ranges {
                advance(&mut trial, r).map_err(SessionError::Gap)?;
            }
            return Ok(());
        }
        match ranges.iter().find(|r| !covered(&self.wanted, r)) {
            Some(stray) => Err(SessionError::Unrequested(*stray)),
            None => Ok(()),
        }
    }

    /// `Importing → Wanting | Idle`: the caller reports what it durably committed; heads advance
    /// and the `Ack` to send carries exactly those runs. A run outside the batch is refused.
    /// Wrapper/inner split, same reason as `hello`.
    #[tracing::instrument(skip_all, fields(from = ?self.state))]
    pub(crate) fn committed(
        &mut self,
        workspace: WorkspaceId,
        ranges: &[OriginRange],
    ) -> Result<Message, SessionError> {
        let r = self.committed_inner(workspace, ranges);
        log_state_result("workspace_committed", &r, self.state);
        r
    }

    fn committed_inner(
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

/// Split out so the event macro doesn't count against the caller's own `#[instrument]` budget
/// (`tasks/logging-sync-crate/notes.md`). Covers `hello`/`on_hello`/`committed`, which all return a
/// `Message` — `to` is `self.state` read back *after* the call (the new state on success, unchanged
/// on failure), branch-free so a `match`/`if` inside a `tracing` macro call doesn't itself cost
/// `cognitive_complexity` points. `SessionError::Crypto` never reaches this helper (only `on_ops`
/// produces it, logged separately by `log_ops_result` at `warn!`).
fn log_state_result(op: &'static str, r: &Result<Message, SessionError>, to: SessionState) {
    tracing::debug!(
        ok = r.is_ok(),
        to = ?to,
        kind = r.as_ref().err().map(SessionError::kind),
        op
    );
}

/// `on_ops`'s own result logger: logs an op count on success (state is always `Importing` by then,
/// nothing `from` didn't already say) and promotes a crypto refusal to `warn!` — the one outcome in
/// this state machine worth a human's attention at a glance, per the backlog line's own "crypto
/// refusal" framing (`tasks/logging-sync-crate/notes.md`). Every other refusal here is routine,
/// validated-not-asserted traffic, so it stays at `debug!`. Each arm delegates to its own one-line,
/// branch-free leaf so the `match` itself (no macro calls of its own) stays cheap and the level
/// choice (`debug!` vs `warn!`) — which a field can't express, unlike `log_state_result`'s `Option`
/// trick — doesn't reintroduce the nested-macro-in-a-branch cost that blew the budget earlier.
fn log_ops_result(r: &Result<Vec<Op>, SessionError>) {
    match r {
        Ok(ops) => log_ops_ok(ops.len()),
        Err(SessionError::Crypto(e)) => log_ops_crypto_refused(e.kind()),
        Err(e) => log_ops_refused(e.kind()),
    }
}

fn log_ops_ok(count: usize) {
    tracing::debug!(count, "workspace_ops_received");
}

fn log_ops_crypto_refused(kind: &'static str) {
    tracing::warn!(kind, "workspace_ops_crypto_refused");
}

fn log_ops_refused(kind: &'static str) {
    tracing::debug!(kind, "workspace_ops_refused");
}

/// `pub(crate)`: `session.rs`'s own link-level `on_link_hello` reuses this to name a wrong-variant
/// message the same way this workspace-level state machine already does.
pub(crate) fn name_of(msg: &Message) -> &'static str {
    match msg {
        Message::Hello { .. } => "Hello",
        Message::Want { .. } => "Want",
        Message::Ops { .. } => "Ops",
        Message::Ack { .. } => "Ack",
        Message::Greet { .. } => "Greet",
    }
}
