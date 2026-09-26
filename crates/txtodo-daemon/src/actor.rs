//! One `FileActor` per synced document: the single writer (design §4.3; M3 crash-recovery startup
//! is `recover`'s own doc, `external.rs`).

use crate::actor_mirror::loro_peer;
use crate::clock::Clock;
use crate::expected::{ExpectedWrites, Hash, hex8};
use crate::external::tracing_stub_error;
use crate::handle::{
    ACTOR_MAILBOX_CAP, ActorError, ActorHandle, ActorMsg, Change, Contents, WATCH_CAP,
};
use crate::mirror::Mirror;
use crate::state::DocState;
use crate::tree_dirty::TreeDirty;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio::sync::{broadcast, mpsc};
use txtodo_core::File;
use txtodo_model::{DeviceId, FilePath, Hlc, IdentityMode, Op, OpId, OpKind, Principal};
use txtodo_store::{Projection, Store};

/// A checkpoint is written every this many ops (design §4.4 "every N ops").
pub const SNAPSHOT_EVERY_OPS: i64 = 500;

/// The workspace's one SQLite store, shared by every actor and the History RPC.
pub type SharedStore = Arc<Mutex<Store>>;

/// blake3 of a projection.
pub fn hash_of(bytes: &[u8]) -> Hash {
    *blake3::hash(bytes).as_bytes()
}

/// Where a document lives.
#[derive(Debug, Clone)]
pub struct ActorConfig {
    /// Workspace-relative path.
    pub path: FilePath,
    /// Absolute path on disk.
    pub disk: PathBuf,
    /// This device.
    pub device: DeviceId,
    /// Shared Health counters.
    pub stats: Arc<crate::stats::Stats>,
    /// How this document establishes task identity (fixed for the workspace's lifetime).
    pub identity_mode: IdentityMode,
    /// Shared workspace-tree staleness flag (plan M5, `tree.rs`).
    pub tree_dirty: Arc<TreeDirty>,
    /// Where this workspace keeps its ref dirs (task workspace-layout), shared with the workspace.
    pub layout: crate::layout_state::SharedLayout,
}

pub(crate) use crate::commit::{Commit, CommitTail};

// Both split out so the event macro doesn't count against the `#[instrument]`ed caller's budget.
fn log_commit_done(change: &Change) {
    tracing::debug!(hash = %hex8(&change.hash), ops = change.ops.len(), "commit_done");
}
fn log_persisted(range: Option<txtodo_store::SeqRange>) {
    tracing::debug!(seq = ?range.map(|r| r.last.0), "persisted");
}

/// `handle_core`'s span field: the variant's name, `"sync"` for `handle_sync`'s group — no payload.
fn actor_msg_kind(msg: &ActorMsg) -> &'static str {
    match msg {
        ActorMsg::ExternalChange => "external_change",
        ActorMsg::Apply { .. } => "apply",
        ActorMsg::Preview { .. } => "preview",
        ActorMsg::Get { .. } => "get",
        ActorMsg::Progress { .. } => "progress",
        ActorMsg::Subscribe { .. } => "subscribe",
        ActorMsg::Undo { .. } => "undo",
        ActorMsg::Import { .. } => "import",
        _ => "sync",
    }
}

/// The actor.
pub struct FileActor {
    pub(crate) cfg: ActorConfig,
    pub(crate) state: DocState,
    /// The Loro merge engine fed every committed op (see `mirror.rs`); derived, never the truth.
    pub(crate) mirror: Mirror,
    pub(crate) projection: Vec<u8>,
    pub(crate) hash: Hash,
    pub(crate) hlc: Hlc,
    pub(crate) store: SharedStore,
    pub(crate) clock: Arc<dyn Clock>,
    pub(crate) expected: ExpectedWrites,
    pub(crate) changes: broadcast::Sender<Change>,
    pub(crate) writes_total: u64,
}

impl FileActor {
    /// Opens the document, recovering from the store and the file as described in the module doc.
    pub fn open(
        cfg: ActorConfig,
        store: SharedStore,
        clock: Arc<dyn Clock>,
    ) -> Result<FileActor, ActorError> {
        let (changes, _) = broadcast::channel(WATCH_CAP);
        let empty =
            DocState::from_file(cfg.path.clone(), &File::default(), &[], cfg.identity_mode)?;
        let mirror = Mirror::from_state(&empty, loro_peer(cfg.device))
            .map_err(|e| ActorError::Mirror(e.to_string()))?;
        let mut actor = FileActor {
            hlc: Hlc::zero(cfg.device),
            state: empty,
            mirror,
            projection: Vec::new(),
            hash: hash_of(&[]),
            cfg,
            store,
            clock,
            expected: ExpectedWrites::default(),
            changes,
            writes_total: 0,
        };
        actor.recover()?;
        debug_assert_eq!(
            actor.hash,
            hash_of(&actor.projection),
            "hash tracks the projection"
        );
        debug_assert_eq!(
            actor.state.to_bytes(),
            actor.projection,
            "state tracks the projection"
        );
        Ok(actor)
    }

    /// Starts the mailbox loop and returns the handle.
    pub fn spawn(self) -> ActorHandle {
        let (tx, rx) = mpsc::channel(ACTOR_MAILBOX_CAP);
        let handle = ActorHandle::new(self.cfg.path.clone(), tx);
        tokio::spawn(self.run(rx));
        handle
    }

    async fn run(mut self, mut rx: mpsc::Receiver<ActorMsg>) {
        // Bounded by the mailbox: the loop ends when every sender is gone, or at `Stop`.
        while let Some(msg) = rx.recv().await {
            if matches!(msg, ActorMsg::Stop) {
                break;
            }
            self.handle(msg);
        }
    }

    fn handle(&mut self, msg: ActorMsg) {
        // Handled here, not in `handle_core`, purely to keep that function's line count in budget.
        if let ActorMsg::Progress { reply } = msg {
            let _ = reply.send(self.state.task_counts());
            return;
        }
        let Some(msg) = self.handle_refdir(msg) else {
            return;
        };
        if let Some(msg) = crate::notes_lookup::handle_task_line(self, msg) {
            self.handle_core(msg);
        }
    }

    /// One event per mailbox message, the whole state machine for free — a thin wrapper, since
    /// `#[instrument]` on `handle_core_match` itself pushes that match over budget.
    #[tracing::instrument(skip_all, fields(file = %self.cfg.path, msg = actor_msg_kind(&msg)))]
    fn handle_core(&mut self, msg: ActorMsg) {
        self.handle_core_match(msg);
    }

    fn handle_core_match(&mut self, msg: ActorMsg) {
        match msg {
            ActorMsg::ExternalChange => {
                if let Err(e) = self.on_external_change() {
                    tracing_stub_error(&self.cfg.path, &e);
                }
            }
            batch @ (ActorMsg::Apply { .. } | ActorMsg::Preview { .. }) => self.handle_batch(batch),
            ActorMsg::Get { reply } => {
                let _ = reply.send(Contents {
                    bytes: self.projection.clone(),
                    hash: self.hash,
                    task_ids: self.state.line_ids().collect(),
                });
            }
            // Handled in `handle`, before this function is reached — never here.
            ActorMsg::Progress { .. } => {}
            ActorMsg::Subscribe { reply } => {
                let _ = reply.send(self.changes.subscribe());
            }
            ActorMsg::Undo {
                steps,
                principal,
                reply,
            } => {
                let _ = reply.send(self.on_undo(steps, principal));
            }
            ActorMsg::Checkout { at_wall_ms, reply } => {
                let _ = reply.send(self.on_checkout(at_wall_ms));
            }
            ActorMsg::Import {
                updates,
                peer,
                reply,
            } => {
                let _ = reply.send(self.on_import(updates, peer));
            }
            // `handle_sync` (import.rs) takes the rest: Conflicts/Version/Export/Resolve, etc.
            other => self.handle_sync(other),
        }
    }

    /// Files written so far (Health, tests).
    pub fn writes_total(&self) -> u64 {
        self.writes_total
    }

    pub(crate) fn lock_store(&self) -> std::sync::MutexGuard<'_, Store> {
        // A poisoned lock still has consistent data (SQLite transactions); keep going.
        self.store
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// One HLC tick per batch, before it applies; a failed batch has harmlessly spent a tick.
    pub(crate) fn tick(&mut self) -> Result<Hlc, ActorError> {
        let before = self.hlc;
        let hlc = self.hlc.tick(self.clock.now_ms())?;
        debug_assert!(hlc > before, "tick is strictly monotone");
        debug_assert_eq!(hlc.device, self.cfg.device);
        Ok(hlc)
    }

    /// One op of the batch stamped `hlc`; every op gets its own id.
    pub(crate) fn stamped(&self, kind: OpKind, hlc: Hlc, principal: &Principal) -> Op {
        debug_assert_eq!(hlc.device, self.cfg.device, "ops carry this device's stamp");
        let op = Op {
            id: OpId::new(self.clock.new_ulid()),
            hlc,
            principal: principal.clone(),
            file: self.cfg.path.clone(),
            kind,
        };
        debug_assert_eq!(op.file, self.cfg.path);
        op
    }

    /// Stamps a whole batch of kinds (one tick) without applying them.
    pub(crate) fn stamp(
        &mut self,
        kinds: Vec<OpKind>,
        principal: &Principal,
    ) -> Result<Vec<Op>, ActorError> {
        if kinds.is_empty() {
            return Ok(Vec::new());
        }
        let hlc = self.tick()?;
        let ops: Vec<Op> = kinds
            .into_iter()
            .map(|kind| self.stamped(kind, hlc, principal))
            .collect();
        debug_assert!(ops.iter().all(|o| o.hlc == hlc), "one stamp per batch");
        Ok(ops)
    }

    #[tracing::instrument(skip_all, fields(file = %self.cfg.path, ops = plan.ops.len()))]
    pub(crate) fn commit(&mut self, plan: Commit) -> Result<Change, ActorError> {
        let change = self.commit_inner(plan)?;
        log_commit_done(&change);
        Ok(change)
    }

    /// Store first (crash-safe: `prev_hash` then points at the still-on-disk bytes, see `recover`).
    fn commit_inner(&mut self, plan: Commit) -> Result<Change, ActorError> {
        let Commit {
            ops,
            next,
            bytes,
            write,
            snapshot,
            tail,
        } = plan;
        let new_hash = hash_of(&bytes);
        let range = self.persist_change(&ops, &bytes, &tail, &next)?;
        self.state = next;
        self.projection = bytes;
        self.hash = new_hash;
        // Disk first: a first mirror flush after a restart materialises the whole Loro snapshot.
        if write {
            self.write_projection_and_log(new_hash)?;
        }
        crate::tree::mark_dirty_for(&self.cfg.tree_dirty, &ops);
        self.update_mirror_after_commit(snapshot, tail.flush, &ops);
        self.raise_flags(&tail.review);
        let change = self.stored_change(new_hash, range, ops, tail.review);
        self.maybe_snapshot(range.map(|r| r.last), snapshot)?;
        self.broadcast(&change);
        debug_assert_eq!(
            self.state.to_bytes(),
            self.projection,
            "state tracks the projection"
        );
        Ok(change)
    }

    /// Lands `ops`/the projection in one store transaction: the durability point.
    #[tracing::instrument(skip_all, fields(file = %self.cfg.path, ops = ops.len()))]
    fn persist_change(
        &mut self,
        ops: &[Op],
        bytes: &[u8],
        tail: &CommitTail,
        next: &DocState,
    ) -> Result<Option<txtodo_store::SeqRange>, ActorError> {
        let projection = Projection {
            file: self.cfg.path.clone(),
            bytes: bytes.to_vec(),
            hash: hash_of(bytes),
            written_at_ms: self.clock.now_ms(),
        };
        let extras = self.commit_extras(tail, next)?;
        let range =
            self.lock_store()
                .commit_change_with(ops, &projection, Some(self.hash), &extras)?;
        log_persisted(range);
        Ok(range)
    }

    /// An adopted state (snapshot) isn't the sum of its ops: converge the mirror, don't feed it.
    fn update_mirror_after_commit(&mut self, snapshot: bool, flush: bool, ops: &[Op]) {
        if snapshot {
            self.converge_mirror();
        } else if flush {
            self.flush_mirror(ops);
        }
    }
}
