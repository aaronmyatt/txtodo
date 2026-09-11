//! One `FileActor` per synced document: the single writer (design §4.3). It owns the in-memory
//! state, the projection bytes and their hash; every path to disk goes through `commit`. Clients
//! talk to it through `ActorHandle`; the watcher sends `ExternalChange`.
//!
//! Startup (plan M3 crash safety): the store's projection and `prev_hash` say whether the file is
//! ours, an interrupted write (disk == prev_hash → finish the write) or a foreign edit (reconcile).

use crate::clock::Clock;
use crate::expected::{ExpectedWrites, Hash};
use crate::handle::{
    ACTOR_MAILBOX_CAP, ActorError, ActorHandle, ActorMsg, Applied, Change, Contents, WATCH_CAP,
};
use crate::mutation::{MAX_MUTATIONS_PER_APPLY, Mutation, MutationError, mutation_ops};
use crate::state::DocState;
use crate::write::write_atomic;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio::sync::{broadcast, mpsc};
use txtodo_core::File;
use txtodo_model::{DeviceId, FilePath, Hlc, Op, OpId, OpKind, Principal, TaskId};
use txtodo_store::{Projection, Seq, Store, Stored};

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
}

/// The actor.
pub struct FileActor {
    pub(crate) cfg: ActorConfig,
    pub(crate) state: DocState,
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
        let mut actor = FileActor {
            hlc: Hlc::zero(cfg.device),
            state: empty,
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
        let mut kinds = Vec::new();
        let clock = Arc::clone(&self.clock);
        let mut mint = || TaskId::new(clock.new_ulid());
        for m in &mutations {
            for kind in mutation_ops(&next, m, &mut mint)? {
                next.apply(&kind)?;
                kinds.push(kind);
            }
        }
        let bytes = next.to_bytes();
        let ops = self.stamp(kinds, principal)?;
        let write = bytes != self.projection;
        let change = self.commit(Commit {
            ops,
            next,
            bytes,
            write,
            snapshot: false,
        })?;
        let applied = u32::try_from(change.ops.len()).unwrap_or(u32::MAX);
        Ok(Applied {
            applied,
            hash: change.hash,
            hlc: self.hlc,
        })
    }

    /// One HLC tick per batch; every op gets its own id.
    pub(crate) fn stamp(
        &mut self,
        kinds: Vec<OpKind>,
        principal: Principal,
    ) -> Result<Vec<Op>, ActorError> {
        if kinds.is_empty() {
            return Ok(Vec::new());
        }
        let hlc = self.hlc.tick(self.clock.now_ms())?;
        let file = self.cfg.path.clone();
        let ops = kinds
            .into_iter()
            .map(|kind| Op {
                id: OpId::new(self.clock.new_ulid()),
                hlc,
                principal: principal.clone(),
                file: file.clone(),
                kind,
            })
            .collect::<Vec<_>>();
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
        } = plan;
        let new_hash = hash_of(&bytes);
        let projection = Projection {
            file: self.cfg.path.clone(),
            bytes: bytes.clone(),
            hash: new_hash,
            written_at_ms: self.clock.now_ms(),
        };
        let range = self
            .lock_store()
            .commit_change(&ops, &projection, Some(self.hash))?;
        self.state = next;
        self.projection = bytes;
        self.hash = new_hash;
        if write {
            self.write_projection()?;
        }
        let first = range.map_or(0, |r| r.first.0);
        let stored: Vec<Stored> = ops
            .into_iter()
            .enumerate()
            .map(|(i, op)| Stored {
                seq: Seq(first + i as i64),
                op,
            })
            .collect();
        self.maybe_snapshot(range.map(|r| r.last), snapshot)?;
        let change = Change {
            path: self.cfg.path.clone(),
            hash: new_hash,
            ops: stored,
        };
        if self.changes.receiver_count() > 0 {
            // Err only when no receiver is left, which the guard above excludes.
            let _ = self.changes.send(change.clone());
        }
        debug_assert_eq!(
            self.state.to_bytes(),
            self.projection,
            "state tracks the projection"
        );
        Ok(change)
    }

    pub(crate) fn write_projection(&mut self) -> Result<(), ActorError> {
        self.expected.arm(self.hash, self.clock.now_instant());
        write_atomic(&self.cfg.disk, &self.projection)?;
        self.writes_total += 1;
        self.cfg.stats.count_write();
        debug_assert!(self.writes_total > 0);
        Ok(())
    }
}

/// Placeholder until tasks/daemon-tracing-logs lands: an actor error with nobody to reply to.
fn tracing_stub_error(path: &FilePath, e: &ActorError) {
    debug_assert!(!path.as_str().is_empty());
    let _ = (path, e);
}
