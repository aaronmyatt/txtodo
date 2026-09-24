//! Long-lived sessions and push on commit (task `sync-live-push`, decided 2026-09-23). After the
//! first `Greet`/`Want`/`Ops`/`Ack` exchange a connection stays open: the driver polls the link
//! every [`POLL`], pushes each routed workspace's new ops to the peer as soon as a commit lands, and
//! sends an empty `Ack` as a heartbeat every [`HEARTBEAT`]. It ends when the peer closes, when the
//! peer has been silent for [`DEAD_AFTER`], or, if no workspace is shared at all, after the old
//! short idle ([`QUIET_CLOSE`]), exactly as before this task.
//!
//! What to push is a head diff, never a queue: per workspace, `Live` tracks what the peer holds
//! (its `Greet`, plus every run it asked for, sent us, or we pushed), and a push is
//! `want(peer holds, local heads)` served through the same `serve_want` a `Want` uses. Ops the peer
//! gave us are already in "peer holds", so nothing echoes back. A workspace is pushed to only
//! once the peer's `Want` for it was served: the stream is ordered, so the pushed batch lands after
//! the batches it asked for, when its session is back in `Idle` and accepts a push.

use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, Instant};

use txtodo_model::DeviceId;
use txtodo_store::WorkspaceId;
use txtodo_sync::{DeviceSigningKey, GroupId, GroupKey, Heads, Link, Message, OriginRange, want};

use crate::device_relay::WorkspaceRoute;
use crate::lan_apply::serve_want;
use crate::lan_session::{read, read_heads};
use crate::lan_session_shared::send_message;
use crate::live_peers::{LiveGuard, LivePeers};

/// How long one wait for a frame lasts: the most a local commit waits before it is pushed.
pub(crate) const POLL: Duration = Duration::from_millis(50);
/// How often an empty `Ack` tells the peer this side is alive, and how often every shared
/// workspace is re-diffed even without a counted commit (a `notes.md` edit, say).
pub(crate) const HEARTBEAT: Duration = Duration::from_secs(5);
/// Silence after which the peer is taken for gone and the session ends (four missed heartbeats).
pub(crate) const DEAD_AFTER: Duration = Duration::from_secs(20);
/// With no workspace shared, a session ends after this much silence: the pre-push short session.
const QUIET_CLOSE: Duration = Duration::from_millis(750);

/// The group crypto and routing a push needs, borrowed from the dispatch loop's own context.
pub(crate) struct PushCtx<'a> {
    pub(crate) group: GroupId,
    pub(crate) key: &'a GroupKey,
    pub(crate) signing_key: &'a DeviceSigningKey,
    pub(crate) routes: &'a BTreeMap<WorkspaceId, WorkspaceRoute>,
}

/// One connection's push and liveness state.
pub(crate) struct Live {
    /// Per workspace, the highest seq the peer holds per origin device, as far as we know.
    peer_heads: BTreeMap<WorkspaceId, Heads>,
    /// Workspaces whose peer `Want` was served: safe to push to.
    ready: BTreeSet<WorkspaceId>,
    /// `Stats::commits` per workspace at its last diff.
    seen_commits: BTreeMap<WorkspaceId, u64>,
    last_heard: Instant,
    last_beat: Instant,
    last_sweep: Instant,
    /// This session's mark on the device's live peers, once the peer's `Hello` named it.
    guard: Option<LiveGuard>,
}

impl Live {
    pub(crate) fn new() -> Live {
        let now = Instant::now();
        Live {
            peer_heads: BTreeMap::new(),
            ready: BTreeSet::new(),
            seen_commits: BTreeMap::new(),
            last_heard: now,
            last_beat: now,
            last_sweep: now,
            guard: None,
        }
    }

    /// A frame arrived.
    pub(crate) fn heard(&mut self) {
        self.last_heard = Instant::now();
    }

    /// Marks `peer` live for as long as this session runs.
    pub(crate) fn enter(&mut self, peers: &LivePeers, peer: DeviceId) {
        if self.guard.is_none() {
            self.guard = Some(peers.enter(peer));
        }
    }

    /// Bookkeeping from one message the peer sent for `workspace`, taken before it is handled.
    pub(crate) fn observe(&mut self, workspace: WorkspaceId, msg: &Message) {
        match msg {
            Message::Greet { heads, .. } => {
                self.peer_heads.insert(workspace, heads.clone());
            }
            Message::Want { ranges, .. } => {
                self.note_holds(workspace, ranges);
                self.ready.insert(workspace);
            }
            Message::Ops { ranges, .. } => self.note_holds(workspace, ranges),
            Message::Ack { .. } | Message::Hello { .. } => {}
        }
    }

    fn note_holds(&mut self, workspace: WorkspaceId, ranges: &[OriginRange]) {
        let heads = self.peer_heads.entry(workspace).or_default();
        for r in ranges {
            let held = heads.entry(r.device).or_insert(0);
            *held = (*held).max(r.last);
        }
    }

    /// One turn after a frame or a quiet poll: push what changed, beat, check the peer is alive.
    /// `false` ends the session.
    pub(crate) fn tick(&mut self, link: &mut dyn Link, ctx: &PushCtx<'_>) -> bool {
        let now = Instant::now();
        if self.expired(now) {
            return log_session_ended(self.ready.len(), now.duration_since(self.last_heard));
        }
        let sweep = now.duration_since(self.last_sweep) >= HEARTBEAT;
        if sweep {
            self.last_sweep = now;
        }
        let ready: Vec<WorkspaceId> = self.ready.iter().copied().collect();
        if !ready.iter().all(|id| self.push(link, ctx, *id, sweep)) {
            return false;
        }
        if now.duration_since(self.last_beat) < HEARTBEAT {
            return true;
        }
        self.last_beat = now;
        self.beat(link, ctx, &ready)
    }

    fn expired(&self, now: Instant) -> bool {
        let quiet = now.duration_since(self.last_heard);
        if self.ready.is_empty() {
            quiet >= QUIET_CLOSE
        } else {
            quiet >= DEAD_AFTER
        }
    }

    /// Pushes `id`'s ops the peer lacks, when a commit landed since the last look (or on a sweep).
    /// `false` only on a failed send.
    fn push(
        &mut self,
        link: &mut dyn Link,
        ctx: &PushCtx<'_>,
        id: WorkspaceId,
        sweep: bool,
    ) -> bool {
        let Some(route) = ctx.routes.get(&id) else {
            return true;
        };
        let commits = read(&route.ws).stats().commits();
        if !sweep && self.seen_commits.get(&id) == Some(&commits) {
            return true;
        }
        self.seen_commits.insert(id, commits);
        let held = self.peer_heads.get(&id).cloned().unwrap_or_default();
        let ranges = want(&held, &read_heads(&route.ws));
        if ranges.is_empty() {
            return true;
        }
        let batches = match serve_want(&route.ws, &ranges, id, ctx.signing_key) {
            Ok(b) => b,
            Err(e) => return log_push_serve_failed(id, &e),
        };
        for batch in batches {
            if send_message(link, ctx.group, id, ctx.key, batch).is_err() {
                return false;
            }
        }
        self.note_holds(id, &ranges);
        log_pushed(id, ranges.len());
        true
    }

    /// An empty `Ack` on a shared workspace: harmless to the peer (it only logs an `Ack`), and it
    /// resets the peer's silence clock. No shared workspace means no heartbeat.
    fn beat(&self, link: &mut dyn Link, ctx: &PushCtx<'_>, ready: &[WorkspaceId]) -> bool {
        let Some(id) = ready.first().copied() else {
            return true;
        };
        let ack = Message::Ack {
            workspace: id.ulid().to_u128(),
            committed: Vec::new(),
        };
        send_message(link, ctx.group, id, ctx.key, ack).is_ok()
    }
}

fn log_session_ended(shared: usize, quiet: Duration) -> bool {
    tracing::debug!(
        shared,
        quiet_ms = quiet.as_millis(),
        "lan_live_session_quiet_ended"
    );
    false
}

fn log_push_serve_failed(workspace: WorkspaceId, e: &txtodo_store::StoreError) -> bool {
    tracing::warn!(%workspace, error = %e, "lan_push_serve_failed");
    true
}

fn log_pushed(workspace: WorkspaceId, runs: usize) {
    tracing::debug!(%workspace, runs, "lan_ops_pushed");
}
