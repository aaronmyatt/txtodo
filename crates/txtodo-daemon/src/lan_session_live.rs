//! Long-lived sessions and push on commit (task `sync-live-push`, decided 2026-09-23). After the
//! first `Greet`/`Want`/`Ops`/`Ack` exchange a connection stays open: the driver polls the link
//! every [`POLL`], pushes each routed workspace's new ops to the peer as soon as a commit lands, and
//! sends an empty `Ack` as a heartbeat every [`HEARTBEAT`]. It ends when the peer closes, when the
//! peer has been silent for [`DEAD_AFTER`], or, if no workspace is shared at all, after the old
//! short idle ([`QUIET_CLOSE`]), exactly as before this task.
//!
//! What to push is a head diff, never a queue: per workspace, `Live` tracks what the peer holds
//! for sure (`held`: its `Greet`, every run it sent us, every run it acked) and, apart from that,
//! what we have sent it since (`sent`). A push is `want(sent, local heads)` served through the same
//! `serve_want` a `Want` uses, so nothing in flight goes twice. Ops the peer gave us are held by
//! definition, so nothing echoes back. A workspace is pushed to only once the peer's `Want` for it
//! was served: the stream is ordered, so the pushed batch lands after the batches it asked for.
//!
//! A run counts as held only once the peer acks it (task `sync-ack-before-held`, 2026-09-25). It
//! used to count as held the moment it was sent, so a batch the peer refused was never sent again
//! in that session. Now, with runs in flight and no ack progress for [`RESEND_AFTER`], `sent`
//! rewinds to `held` and the next diff sends them again. A copy that lands anyway is out of step
//! with the peer's heads and skipped there (`lan_session_shared.rs`).
//!
//! Workspaces take turns (task `sync-link-fairness`, 2026-09-25): a turn sends at most one batch
//! per workspace, and none while [`WINDOW_BATCHES`] are unacked, so a small change in one workspace
//! never queues behind another's thousands of ops. A peer's `Want` is served here too, a batch a
//! turn, up to the heads it asked for, before anything newer is pushed.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use txtodo_model::DeviceId;
use txtodo_store::WorkspaceId;
use txtodo_sync::{
    DeviceSigningKey, GroupId, GroupKey, Heads, Link, MAX_OPS_PER_BATCH, Message, OriginRange, want,
};

use crate::clock::Clock;
use crate::device_relay::WorkspaceRoute;
use crate::lan_apply::serve_want;
use crate::lan_session::{read, read_heads};
use crate::lan_session_shared::send_message;
use crate::live_peers::{Carrier, LiveGuard, LivePeers};

/// How long one wait for a frame lasts: the most a local commit waits before it is pushed.
pub(crate) const POLL: Duration = Duration::from_millis(50);
/// How often an empty `Ack` tells the peer this side is alive, and how often every shared
/// workspace is re-diffed even without a counted commit (a `notes.md` edit, say).
pub(crate) const HEARTBEAT: Duration = Duration::from_secs(5);
/// Silence after which the peer is taken for gone and the session ends (four missed heartbeats).
pub(crate) const DEAD_AFTER: Duration = Duration::from_secs(20);
/// With no workspace shared, a session ends after this much silence: the pre-push short session.
const QUIET_CLOSE: Duration = Duration::from_millis(750);
/// Runs in flight with no ack progress for this long are sent again: two heartbeats, well under
/// [`DEAD_AFTER`], so a live but refusing peer gets them again before the session is written off.
#[cfg(not(test))]
pub(crate) const RESEND_AFTER: Duration = Duration::from_secs(10);
/// Short under test, so a resend test does not wait ten seconds.
#[cfg(test)]
pub(crate) const RESEND_AFTER: Duration = Duration::from_millis(300);
/// Most unacked batches in flight per workspace: enough to keep the link busy while the peer
/// commits one, few enough that another workspace's batch is never far behind in the queue.
pub(crate) const WINDOW_BATCHES: u64 = 2;

/// The group crypto and routing a push needs, borrowed from the dispatch loop's own context.
pub(crate) struct PushCtx<'a> {
    pub(crate) group: GroupId,
    pub(crate) key: &'a GroupKey,
    pub(crate) signing_key: &'a DeviceSigningKey,
    pub(crate) routes: &'a BTreeMap<WorkspaceId, WorkspaceRoute>,
}

/// One connection's push and liveness state.
pub(crate) struct Live {
    /// Per workspace, the highest seq the peer holds per origin device for sure: its `Greet`, the
    /// runs it sent us, the runs it acked.
    held: BTreeMap<WorkspaceId, Heads>,
    /// Per workspace, `held` plus every run we sent since and the peer has not acked yet: what a
    /// push diffs against.
    sent: BTreeMap<WorkspaceId, Heads>,
    /// Per workspace with runs in flight: when the first went out, or the peer last acked one.
    waiting_since: BTreeMap<WorkspaceId, Instant>,
    /// Workspaces whose peer sent its `Want`: safe to push to.
    ready: BTreeSet<WorkspaceId>,
    /// Per workspace, the heads the peer's `Want` asked for, until they are sent: served first.
    asked: BTreeMap<WorkspaceId, Heads>,
    /// Workspaces with more to send than the last turn sent.
    pending: BTreeSet<WorkspaceId>,
    /// `Stats::commits` per workspace at its last diff, and the local heads read then.
    seen_commits: BTreeMap<WorkspaceId, (u64, Heads)>,
    last_heard: Instant,
    last_beat: Instant,
    last_sweep: Instant,
    /// This session's mark on the device's live peers, once the peer's `Hello` named it.
    guard: Option<LiveGuard>,
    /// Injected time (stack.md idiom): every `Instant` this session compares against comes from
    /// here, never `Instant::now()` directly, so a test's `FakeClock` makes `RESEND_AFTER`/
    /// `HEARTBEAT`/`DEAD_AFTER` deterministic instead of racing real wall-clock CI slowness
    /// (found flaky on a loaded coverage runner, task `sync-link-fairness`'s own follow-up).
    clock: Arc<dyn Clock>,
}

impl Live {
    pub(crate) fn new(clock: Arc<dyn Clock>) -> Live {
        let now = clock.now_instant();
        Live {
            held: BTreeMap::new(),
            sent: BTreeMap::new(),
            waiting_since: BTreeMap::new(),
            ready: BTreeSet::new(),
            asked: BTreeMap::new(),
            pending: BTreeSet::new(),
            seen_commits: BTreeMap::new(),
            last_heard: now,
            last_beat: now,
            last_sweep: now,
            guard: None,
            clock,
        }
    }

    /// A frame arrived.
    pub(crate) fn heard(&mut self) {
        self.last_heard = self.clock.now_instant();
    }

    /// Marks `peer` live over `carrier` for as long as this session runs.
    pub(crate) fn enter(&mut self, peers: &LivePeers, peer: DeviceId, carrier: Carrier) {
        if self.guard.is_none() {
            self.guard = Some(peers.enter(peer, carrier));
        }
    }

    /// A relay session whose peer now has a LAN session too (task lan-dial-falls-to-relay).
    fn superseded(&self) -> bool {
        self.guard.as_ref().is_some_and(|g| {
            let (peer, carrier) = g.key();
            carrier == Carrier::Relay && g.peers().is_live_on(peer, Carrier::Lan)
        })
    }

    /// Bookkeeping from one message the peer sent for `workspace`, taken before it is handled.
    pub(crate) fn observe(&mut self, workspace: WorkspaceId, msg: &Message) {
        match msg {
            Message::Greet { heads, .. } => {
                self.held.insert(workspace, heads.clone());
                self.sent.insert(workspace, heads.clone());
                self.waiting_since.remove(&workspace);
            }
            Message::Want { ranges, .. } => {
                // Served by `tick`, a batch a turn, ahead of any push (task sync-link-fairness).
                let mut asked = self.sent.get(&workspace).cloned().unwrap_or_default();
                raise(&mut asked, ranges);
                self.asked.insert(workspace, asked);
                self.ready.insert(workspace);
                self.pending.insert(workspace);
            }
            Message::Ops { ranges, .. } => {
                raise(self.held.entry(workspace).or_default(), ranges);
                raise(self.sent.entry(workspace).or_default(), ranges);
            }
            Message::Ack { committed, .. } => self.note_acked(workspace, committed),
            Message::Hello { .. } => {}
        }
    }

    fn note_sent(&mut self, workspace: WorkspaceId, ranges: &[OriginRange]) {
        if ranges.is_empty() {
            return;
        }
        raise(self.sent.entry(workspace).or_default(), ranges);
        let now = self.clock.now_instant();
        self.waiting_since.entry(workspace).or_insert(now);
    }

    /// An `Ack`: its runs are held now. Any progress restarts the resend clock; an empty `Ack` (a
    /// heartbeat, or a batch that committed nothing) does not.
    fn note_acked(&mut self, workspace: WorkspaceId, committed: &[OriginRange]) {
        if committed.is_empty() {
            return;
        }
        raise(self.held.entry(workspace).or_default(), committed);
        let held = self.held.get(&workspace).cloned().unwrap_or_default();
        raise_to(self.sent.entry(workspace).or_default(), &held);
        if self.in_flight(workspace) {
            let now = self.clock.now_instant();
            self.waiting_since.insert(workspace, now);
        } else {
            self.waiting_since.remove(&workspace);
        }
    }

    /// Whether we sent `workspace` runs the peer has not acked.
    fn in_flight(&self, workspace: WorkspaceId) -> bool {
        let (Some(sent), held) = (self.sent.get(&workspace), self.held.get(&workspace)) else {
            return false;
        };
        sent.iter()
            .any(|(d, s)| *s > held.and_then(|h| h.get(d)).copied().unwrap_or(0))
    }

    /// With runs in flight and no ack progress for [`RESEND_AFTER`], forgets them: the next diff
    /// starts from what the peer acked. `true` when it rewound.
    fn rewind_if_stalled(&mut self, workspace: WorkspaceId, now: Instant) -> bool {
        let Some(since) = self.waiting_since.get(&workspace).copied() else {
            return false;
        };
        if now.duration_since(since) < RESEND_AFTER {
            return false;
        }
        self.waiting_since.remove(&workspace);
        if !self.in_flight(workspace) {
            return false;
        }
        let held = self.held.get(&workspace).cloned().unwrap_or_default();
        self.sent.insert(workspace, held);
        log_push_rewound(workspace);
        true
    }

    /// One turn after a frame or a quiet poll: push what changed, beat, check the peer is alive.
    /// `false` ends the session.
    pub(crate) fn tick(&mut self, link: &mut dyn Link, ctx: &PushCtx<'_>) -> bool {
        let now = self.clock.now_instant();
        if self.expired(now) {
            return log_session_ended(self.ready.len(), now.duration_since(self.last_heard));
        }
        if self.superseded() {
            return log_superseded_by_lan();
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

    /// Sends `id`'s next batch the peer lacks, when a commit landed since the last look, a sweep
    /// or a rewind is due, or the last turn left more to send; nothing while the window is full.
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
        let rewound = self.rewind_if_stalled(id, self.clock.now_instant());
        let seen = self.seen_commits.get(&id);
        let fresh = seen.is_none_or(|(c, _)| *c != commits);
        if !(sweep || rewound || fresh || self.pending.contains(&id)) {
            return true;
        }
        let local = match seen {
            Some((c, heads)) if *c == commits && !sweep => heads.clone(),
            _ => read_heads(&route.ws),
        };
        self.seen_commits.insert(id, (commits, local.clone()));
        let Some(batch) = self.next_batch(id, &local) else {
            self.pending.remove(&id);
            return true;
        };
        self.pending.insert(id);
        if self.in_flight_ops(id) >= WINDOW_BATCHES * MAX_OPS_PER_BATCH as u64 {
            return true;
        }
        let messages = match serve_want(&route.ws, &[batch], id, ctx.signing_key) {
            Ok(m) => m,
            Err(e) => return log_push_serve_failed(id, &e),
        };
        for message in messages {
            if send_message(link, ctx.group, id, ctx.key, message).is_err() {
                return false;
            }
        }
        self.note_sent(id, &[batch]);
        log_pushed(id, batch);
        true
    }

    /// The next run to send `id`'s peer, one batch wide: up to what its `Want` asked for while
    /// that is still owed, else up to the local heads.
    fn next_batch(&mut self, id: WorkspaceId, local: &Heads) -> Option<OriginRange> {
        let sent = self.sent.get(&id).cloned().unwrap_or_default();
        let owed = self.asked.get(&id).map(|asked| want(&sent, asked));
        let runs = match owed {
            Some(runs) if !runs.is_empty() => runs,
            _ => {
                self.asked.remove(&id);
                want(&sent, local)
            }
        };
        let first = runs.first()?;
        let width = MAX_OPS_PER_BATCH as u64;
        Some(OriginRange {
            last: first.last.min(first.first + width - 1),
            ..*first
        })
    }

    /// Ops sent to `id`'s peer and not acked yet.
    fn in_flight_ops(&self, id: WorkspaceId) -> u64 {
        let (Some(sent), held) = (self.sent.get(&id), self.held.get(&id)) else {
            return 0;
        };
        sent.iter()
            .map(|(d, s)| s.saturating_sub(held.and_then(|h| h.get(d)).copied().unwrap_or(0)))
            .sum()
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

fn log_superseded_by_lan() -> bool {
    tracing::info!("lan_relay_session_superseded_by_lan");
    false
}

fn log_push_serve_failed(workspace: WorkspaceId, e: &txtodo_store::StoreError) -> bool {
    tracing::warn!(%workspace, error = %e, "lan_push_serve_failed");
    true
}

/// Raises each run's device in `heads` to the run's end: runs follow heads, so the max is the head.
fn raise(heads: &mut Heads, ranges: &[OriginRange]) {
    for r in ranges {
        let head = heads.entry(r.device).or_insert(0);
        *head = (*head).max(r.last);
    }
}

/// Raises every device in `heads` to at least its head in `floor`.
fn raise_to(heads: &mut Heads, floor: &Heads) {
    for (device, h) in floor {
        let head = heads.entry(*device).or_insert(0);
        *head = (*head).max(*h);
    }
}

fn log_push_rewound(workspace: WorkspaceId) {
    tracing::info!(%workspace, "lan_push_rewound_unacked");
}

fn log_pushed(workspace: WorkspaceId, run: OriginRange) {
    tracing::debug!(%workspace, first = run.first, last = run.last, "lan_ops_pushed");
}
