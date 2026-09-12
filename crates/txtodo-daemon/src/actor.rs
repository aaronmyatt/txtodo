//! One `FileActor` per synced document: the single writer (design §4.3). It owns the in-memory
//! state, the projection bytes and their hash; every path to disk goes through `commit`. Clients
//! talk to it through `ActorHandle`; the watcher sends `ExternalChange`.
//!
//! Startup (plan M3 crash safety): the store's projection and `prev_hash` say whether the file is
//! ours, an interrupted write (disk == prev_hash → finish the write) or a foreign edit (reconcile).

use crate::actor_mirror::loro_peer;
use crate::clock::Clock;
use crate::expected::{ExpectedWrites, Hash, hex8};
use crate::external::tracing_stub_error;
use crate::handle::{
    ACTOR_MAILBOX_CAP, ActorError, ActorHandle, ActorMsg, Applied, Change, Contents, WATCH_CAP,
};
use crate::mirror::Mirror;
use crate::mutation::{MAX_MUTATIONS_PER_APPLY, Mutation, MutationError, mutation_ops};
use crate::state::DocState;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio::sync::{broadcast, mpsc};
use txtodo_core::File;
use txtodo_model::{DeviceId, FilePath, Hlc, Op, OpId, OpKind, Principal, TaskId};
use txtodo_store::{Projection, ReviewRow, Seq, Store, Stored};

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
}

/// One persisted change, ready to commit.
pub(crate) struct Commit {
    pub(crate) ops: Vec<Op>,
    pub(crate) next: DocState,
    pub(crate) bytes: Vec<u8>,
    pub(crate) write: bool,
    pub(crate) snapshot: bool,
    pub(crate) tail: CommitTail,
}

/// What a commit does besides landing ops and bytes (plan M4 sync paths).
pub(crate) struct CommitTail {
    /// needs_review flags to raise with this change.
    pub(crate) review: Vec<ReviewRow>,
    /// Feed the ops to the mirror afterwards (false when the mirror already holds them: import).
    pub(crate) flush: bool,
    /// Clear this flag in the store transaction (a resolution).
    pub(crate) clear: Option<(TaskId, u64)>,
    /// Store the mirror snapshot in the store transaction (an import).
    pub(crate) persist_mirror: bool,
}

impl Default for CommitTail {
    fn default() -> CommitTail {
        CommitTail {
            review: Vec::new(),
            flush: true,
            clear: None,
            persist_mirror: false,
        }
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
        let empty = DocState::from_file(cfg.path.clone(), &File::default())?;
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
        // Bounded by the mailbox: the loop ends when every sender is gone.
        while let Some(msg) = rx.recv().await {
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

    fn handle_core(&mut self, msg: ActorMsg) {
        match msg {
            ActorMsg::ExternalChange => {
                if let Err(e) = self.on_external_change() {
                    tracing_stub_error(&self.cfg.path, &e);
                }
            }
            ActorMsg::Apply {
                mutations,
                principal,
                reply,
            } => {
                let _ = reply.send(self.on_apply(mutations, principal));
            }
            ActorMsg::Get { reply } => {
                let _ = reply.send(Contents {
                    bytes: self.projection.clone(),
                    hash: self.hash,
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
            // `handle_sync` (import.rs) takes the rest: Conflicts/Version/Export/Resolve plus
            // every variant `handle`/this match already consumed (never actually reached there).
            other => self.handle_sync(other),
        }
    }

    /// Files written so far (Health, tests).
    pub fn writes_total(&self) -> u64 {
        self.writes_total
    }

    pub(crate) fn lock_store(&self) -> std::sync::MutexGuard<'_, Store> {
        // A poisoned lock means another actor panicked mid-write; the data is still consistent
        // (SQLite transactions), so keep going with the inner value.
        self.store
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn on_apply(
        &mut self,
        mutations: Vec<Mutation>,
        principal: Principal,
    ) -> Result<Applied, ActorError> {
        if mutations.len() > MAX_MUTATIONS_PER_APPLY {
            return Err(MutationError::TooMany(mutations.len()).into());
        }
        let mut next = self.state.clone();
        let mut ops = Vec::new();
        let clock = Arc::clone(&self.clock);
        let mut mint = || TaskId::new(clock.new_ulid());
        let hlc = self.tick()?;
        for m in &mutations {
            for kind in mutation_ops(&next, m, &mut mint)? {
                let op = self.stamped(kind, hlc, &principal);
                next.apply(&op)?;
                ops.push(op);
            }
        }
        let bytes = next.to_bytes();
        let write = bytes != self.projection;
        let change = self.commit(Commit {
            ops,
            next,
            bytes,
            write,
            snapshot: false,
            tail: CommitTail::default(),
        })?;
        let applied = u32::try_from(change.ops.len()).unwrap_or(u32::MAX);
        Ok(Applied {
            applied,
            hash: change.hash,
            hlc: self.hlc,
        })
    }

    /// One HLC tick per batch, taken before the batch is applied so every op carries its stamp
    /// into the state (the M4 store arbitrates fields by it). A batch that then fails has spent a
    /// tick; the clock only ever moves forward, so that is harmless.
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

    /// Persists, then writes, then swaps the in-memory state. Store first so a crash between the
    /// two leaves `prev_hash` pointing at the bytes still on disk (see `recover`).
    pub(crate) fn commit(&mut self, plan: Commit) -> Result<Change, ActorError> {
        let Commit {
            ops,
            next,
            bytes,
            write,
            snapshot,
            tail,
        } = plan;
        let new_hash = hash_of(&bytes);
        let projection = Projection {
            file: self.cfg.path.clone(),
            bytes: bytes.clone(),
            hash: new_hash,
            written_at_ms: self.clock.now_ms(),
        };
        let extras = self.commit_extras(&tail)?;
        let range =
            self.lock_store()
                .commit_change_with(&ops, &projection, Some(self.hash), &extras)?;
        self.state = next;
        self.projection = bytes;
        self.hash = new_hash;
        // Disk first: the file never waits on the mirror (a first flush after a restart
        // materialises the whole Loro snapshot, seconds for 10k tasks in debug).
        if write {
            self.write_projection()?;
            tracing::info!(file = %self.cfg.path, bytes = self.projection.len(), hash = %hex8(&new_hash), "projection_written");
        }
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

    /// An adopted state (snapshot) is not the sum of its ops: converge the mirror instead of
    /// feeding it ops it never actually replayed.
    fn update_mirror_after_commit(&mut self, snapshot: bool, flush: bool, ops: &[Op]) {
        if snapshot {
            self.converge_mirror();
        } else if flush {
            self.flush_mirror(ops);
        }
    }

    /// Numbers the committed ops and assembles the `Change` this commit produced.
    fn stored_change(
        &self,
        hash: Hash,
        range: Option<txtodo_store::SeqRange>,
        ops: Vec<Op>,
        review: Vec<ReviewRow>,
    ) -> Change {
        let first = range.map_or(0, |r| r.first.0);
        let stored: Vec<Stored> = ops
            .into_iter()
            .enumerate()
            .map(|(i, op)| Stored {
                seq: Seq(first + i as i64),
                op,
            })
            .collect();
        Change {
            path: self.cfg.path.clone(),
            hash,
            ops: stored,
            review,
        }
    }

    /// Sends `change` to every `Watch` subscriber; a full mailbox never blocks the commit that
    /// just landed durably (`Err` only means no receiver is left, which the guard excludes).
    fn broadcast(&self, change: &Change) {
        if self.changes.receiver_count() > 0 {
            let _ = self.changes.send(change.clone());
        }
    }
}
