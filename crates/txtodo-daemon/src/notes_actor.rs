//! One `notes.md`'s owner (plan M5): the notes analogue of `FileActor`, deliberately not a
//! `FileActor`/`DocState` (see `notes_state.rs`'s module doc — that reintroduces the id-stamping-
//! into-prose bug `workspace_tests::notes_md_is_left_alone` guards against). Same discipline as
//! every other document in this crate: store first (`Store::commit_change`), then rename
//! (`write::write_atomic`, temp + fsync + rename), and a hash comparison at open time to tell our
//! own projection from a foreign edit.
//!
//! No tokio mailbox: unlike `FileActor`, nothing drives a notes.md from a filesystem watcher in
//! this plan (M5's scope is `GetNotes`/`EditNotes` plus history/undo/checkout — a live
//! external-edit reconciler for notes.md is a natural follow-up, not required here). One writer is
//! instead enforced by `notes_registry.rs`'s `Arc<Mutex<NotesActor>>` per path: every `GetNotes`/
//! `EditNotes` call for the same `ref:` directory serialises through the same lock.

use std::path::PathBuf;
use std::sync::Arc;

use crate::actor::{SharedStore, hash_of};
use crate::actor_mirror::loro_peer;
use crate::clock::Clock;
use crate::expected::Hash;
use crate::handle::{ActorError, Applied};
use crate::history::MAX_REPLAY_PAGES;
use crate::notes_mirror::NotesMirror;
use crate::notes_state::NotesState;
use crate::write::write_atomic;
use std::sync::PoisonError;
use txtodo_model::{DeviceId, FilePath, Hlc, Op, OpId, OpKind, Principal, TextEdit};
use txtodo_store::{MAX_OPS_PER_READ, Projection, Seq, SeqRange};

/// Where a notes document lives.
#[derive(Debug, Clone)]
pub struct NotesActorConfig {
    /// Workspace-relative `<ref>/notes.md`.
    pub path: FilePath,
    /// Absolute path on disk.
    pub disk: PathBuf,
    /// This device.
    pub device: DeviceId,
}

/// One `notes.md`'s in-memory owner.
pub struct NotesActor {
    cfg: NotesActorConfig,
    state: NotesState,
    mirror: NotesMirror,
    projection: Vec<u8>,
    hash: Hash,
    hlc: Hlc,
    store: SharedStore,
    clock: Arc<dyn Clock>,
}

impl NotesActor {
    /// Opens the document from what the store already holds — its projection, or, right after
    /// pairing shipped a mirror snapshot and no projection yet, the mirror's own text — then
    /// reconciles what is on disk against that the way `FileActor::recover` does for `todo.txt`
    /// (task notes-sync): bytes that differ (a file written by hand, an older build, or the very
    /// first open of a non-empty file) are committed as one `NotesEdit` op from this device, so
    /// a peer's `Want` can fetch them. Before this, `open` adopted foreign bytes into `state` with
    /// no op, and a hand-written notes.md never synced. The Loro mirror is restored from its last
    /// persisted snapshot plus the ops since, so lineage survives a restart (design §4.4); with
    /// no snapshot yet it hydrates fresh, a new lineage, same as `Mirror::from_state`'s case.
    pub fn open(
        cfg: NotesActorConfig,
        store: SharedStore,
        clock: Arc<dyn Clock>,
    ) -> Result<NotesActor, ActorError> {
        let disk_bytes = std::fs::read(&cfg.disk).unwrap_or_default();
        let (projection, mirror_snapshot) = {
            let guard = store.lock().unwrap_or_else(PoisonError::into_inner);
            let projection = guard.get_projection(&cfg.path)?.map(|p| p.bytes);
            (projection, guard.get_mirror(&cfg.path)?)
        };
        let peer = loro_peer(cfg.device);
        let (mirror, state_bytes) = match mirror_snapshot {
            Some((snap, since)) => {
                let mirror = Self::restore_mirror(&store, &cfg.path, &snap, since, peer)?;
                let bytes = projection.unwrap_or_else(|| mirror.text().into_bytes());
                (mirror, bytes)
            }
            None => {
                let bytes = projection.unwrap_or_default();
                let state = NotesState::from_bytes(cfg.path.clone(), &bytes)?;
                let mirror = NotesMirror::from_state(&state, peer).map_err(mirror_err)?;
                (mirror, bytes)
            }
        };
        let state = NotesState::from_bytes(cfg.path.clone(), &state_bytes)?;
        let bytes = state.to_bytes();
        let hash = hash_of(&bytes);
        let mut actor = NotesActor {
            hlc: Hlc::zero(cfg.device),
            state,
            mirror,
            projection: bytes,
            hash,
            cfg,
            store,
            clock,
        };
        if hash_of(&disk_bytes) != actor.hash {
            let device = actor.cfg.device;
            let disk_text = String::from_utf8_lossy(&disk_bytes).into_owned();
            actor.edit(&disk_text, Principal::External { device })?;
        }
        Ok(actor)
    }

    /// The persisted mirror plus the ops committed since it was taken (bounded paging, same shape
    /// as `history::replay`'s).
    fn restore_mirror(
        store: &SharedStore,
        path: &FilePath,
        snapshot: &[u8],
        since: Seq,
        peer: u64,
    ) -> Result<NotesMirror, ActorError> {
        let mut mirror = NotesMirror::from_snapshot(snapshot, path, peer).map_err(mirror_err)?;
        let mut since = since;
        for _page in 0..MAX_REPLAY_PAGES {
            let ops = {
                let guard = store.lock().unwrap_or_else(PoisonError::into_inner);
                guard.for_file(path, since)?
            };
            let Some(last) = ops.last() else { break };
            let plain: Vec<Op> = ops.iter().map(|s| s.op.clone()).collect();
            mirror.flush(&plain).map_err(mirror_err)?;
            since = last.seq;
            if ops.len() < MAX_OPS_PER_READ {
                break;
            }
        }
        Ok(mirror)
    }

    /// Current bytes and hash.
    pub fn contents(&self) -> (Vec<u8>, Hash) {
        (self.projection.clone(), self.hash)
    }

    /// Applies a whole-document replacement as one `NotesEdit` op: diffs it against the current
    /// text (`txtodo_core::diff_text`, the same char-level convention `EditText` uses), so
    /// concurrent edits from two devices still merge character-wise once both land in the mirror.
    pub fn edit(&mut self, new_text: &str, principal: Principal) -> Result<Applied, ActorError> {
        let edits: Vec<TextEdit> = txtodo_core::diff_text(self.state.text(), new_text)
            .into_iter()
            .map(TextEdit::from)
            .collect();
        if edits.is_empty() {
            return Ok(self.no_op_applied());
        }
        let hlc = self.tick()?;
        let op = self.stamped(edits, hlc, principal);
        let mut next = self.state.clone();
        next.apply(&op)?;
        self.commit(vec![op], next)?;
        Ok(Applied {
            applied: 1,
            hash: self.hash,
            hlc,
        })
    }

    /// Merges a peer's Loro updates and appends one local log entry that reproduces the merge on
    /// replay (the text before/after the import, diffed — see `notes_mirror.rs`'s module doc).
    pub fn import_updates(
        &mut self,
        updates: &[u8],
        peer: DeviceId,
    ) -> Result<Applied, ActorError> {
        let (before, after) = self.mirror.import(updates).map_err(mirror_err)?;
        if before == after {
            return Ok(self.no_op_applied());
        }
        let edits: Vec<TextEdit> = txtodo_core::diff_text(&before, &after)
            .into_iter()
            .map(TextEdit::from)
            .collect();
        let hlc = self.tick()?;
        let op = self.stamped(edits, hlc, Principal::User { device: peer });
        let mut next = self.state.clone();
        next.apply(&op)?;
        self.commit_without_mirror_flush(vec![op], next)?;
        Ok(Applied {
            applied: 1,
            hash: self.hash,
            hlc,
        })
    }

    /// Applies a peer's `NotesEdit` ops (task notes-sync), in the order they arrived, the way
    /// `FileActor::on_sync_ops` applies a peer's task ops: the batch commits whole or not at all,
    /// and the merged text lands in the store, on disk and in the Loro mirror. Ops for another
    /// path are refused by `NotesState::apply`.
    pub fn import_ops(&mut self, ops: Vec<Op>) -> Result<(), ActorError> {
        if ops.is_empty() {
            return Ok(());
        }
        let mut next = self.state.clone();
        for op in &ops {
            next.apply(op)?;
        }
        self.commit(ops, next)
    }

    /// The mirror's version, for a peer to export updates since.
    pub fn version(&self) -> Vec<u8> {
        self.mirror.version()
    }

    /// The mirror's current snapshot, for seeding a fresh peer's `Store::put_mirror` (what pairing
    /// ships as "snapshot plus ops").
    pub fn mirror_snapshot(&self) -> Vec<u8> {
        self.mirror.snapshot().unwrap_or_default()
    }

    /// The Loro updates a peer at `since` is missing.
    pub fn export_since(&self, since: &[u8]) -> Result<Vec<u8>, ActorError> {
        self.mirror.export_since(since).map_err(mirror_err)
    }

    fn no_op_applied(&self) -> Applied {
        Applied {
            applied: 0,
            hash: self.hash,
            hlc: self.hlc,
        }
    }

    fn tick(&mut self) -> Result<Hlc, ActorError> {
        let before = self.hlc;
        let hlc = self.hlc.tick(self.clock.now_ms())?;
        debug_assert!(hlc > before, "tick is strictly monotone");
        Ok(hlc)
    }

    fn stamped(&self, edits: Vec<TextEdit>, hlc: Hlc, principal: Principal) -> Op {
        Op {
            id: OpId::new(self.clock.new_ulid()),
            hlc,
            principal,
            file: self.cfg.path.clone(),
            kind: OpKind::NotesEdit {
                file: self.cfg.path.clone(),
                edits,
            },
        }
    }

    /// Persists, writes, then feeds the local edit to the mirror directly (we already know the
    /// exact edits — see `NotesMirror::flush`) before persisting its snapshot.
    fn commit(&mut self, ops: Vec<Op>, next: NotesState) -> Result<(), ActorError> {
        let range = self.land(&ops, &next)?;
        self.mirror.flush(&ops).map_err(mirror_err)?;
        self.persist_mirror(range)
    }

    /// Same as `commit`, but the mirror already holds these ops (an import merges there first).
    fn commit_without_mirror_flush(
        &mut self,
        ops: Vec<Op>,
        next: NotesState,
    ) -> Result<(), ActorError> {
        let range = self.land(&ops, &next)?;
        self.persist_mirror(range)
    }

    fn land(&mut self, ops: &[Op], next: &NotesState) -> Result<Option<SeqRange>, ActorError> {
        let bytes = next.to_bytes();
        let new_hash = hash_of(&bytes);
        let projection = Projection {
            file: self.cfg.path.clone(),
            bytes: bytes.clone(),
            hash: new_hash,
            written_at_ms: self.clock.now_ms(),
        };
        let range = {
            let mut store = self.store.lock().unwrap_or_else(PoisonError::into_inner);
            store.commit_change(ops, &projection, Some(self.hash))?
        };
        self.state = next.clone();
        self.projection = bytes;
        self.hash = new_hash;
        write_atomic(&self.cfg.disk, &self.projection)?;
        Ok(range)
    }

    /// Stores the mirror's current snapshot at this commit's seq, so a restart resumes the same
    /// lineage (`open`'s `restore_mirror`) instead of forking a new one.
    fn persist_mirror(&mut self, range: Option<SeqRange>) -> Result<(), ActorError> {
        let mut store = self.store.lock().unwrap_or_else(PoisonError::into_inner);
        let seq = match range {
            Some(r) => r.last,
            None => store.last_seq()?.unwrap_or(Seq(0)),
        };
        let snapshot = self.mirror.snapshot().map_err(mirror_err)?;
        store.put_mirror(&self.cfg.path, &snapshot, seq)?;
        Ok(())
    }
}

fn mirror_err(e: crate::notes_mirror::NotesMirrorError) -> ActorError {
    ActorError::Mirror(e.to_string())
}

#[cfg(test)]
mod sync_tests {
    use super::*;
    use crate::clock::FakeClock;
    use std::sync::Mutex;
    use txtodo_model::Ulid;
    use txtodo_store::Store;

    fn setup(dir: &std::path::Path, n: u128) -> (SharedStore, Arc<dyn Clock>, NotesActorConfig) {
        let store: SharedStore = Arc::new(Mutex::new(
            Store::open(&dir.join(format!("oplog-{n}.db"))).unwrap_or_else(|e| panic!("{e}")),
        ));
        let clock: Arc<dyn Clock> = Arc::new(FakeClock::new(1_000 + n as u64));
        let cfg = NotesActorConfig {
            path: FilePath::new("tasks/abc/notes.md").unwrap_or_else(|e| panic!("{e}")),
            disk: dir.join(format!("dev{n}/tasks/abc/notes.md")),
            device: DeviceId::new(Ulid::from_u128(n)),
        };
        std::fs::create_dir_all(cfg.disk.parent().unwrap_or(dir)).unwrap_or_else(|e| panic!("{e}"));
        (store, clock, cfg)
    }

    fn ops_for(store: &SharedStore, path: &FilePath) -> Vec<Op> {
        let guard = store.lock().unwrap_or_else(PoisonError::into_inner);
        guard
            .for_file(path, Seq(0))
            .unwrap_or_else(|e| panic!("{e}"))
            .into_iter()
            .map(|s| s.op)
            .collect()
    }

    #[test]
    fn a_hand_written_notes_md_is_seeded_as_one_op_on_open_and_only_once() {
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let (store, clock, cfg) = setup(dir.path(), 1);
        std::fs::write(&cfg.disk, "# Written by hand\n").unwrap_or_else(|e| panic!("{e}"));
        let actor = NotesActor::open(cfg.clone(), Arc::clone(&store), Arc::clone(&clock))
            .unwrap_or_else(|e| panic!("open: {e}"));
        assert_eq!(actor.contents().0, b"# Written by hand\n");
        let ops = ops_for(&store, &cfg.path);
        assert_eq!(ops.len(), 1, "one seed op for the peer to fetch");
        assert!(matches!(ops[0].principal, Principal::External { .. }));
        drop(actor);
        // A reopen with the file unchanged mints nothing more.
        let _again = NotesActor::open(cfg.clone(), Arc::clone(&store), clock)
            .unwrap_or_else(|e| panic!("reopen: {e}"));
        assert_eq!(ops_for(&store, &cfg.path).len(), 1);
    }

    #[test]
    fn a_peers_ops_import_into_a_fresh_actor_and_land_on_disk() {
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let (store_a, clock_a, cfg_a) = setup(dir.path(), 1);
        let mut a = NotesActor::open(cfg_a.clone(), Arc::clone(&store_a), clock_a)
            .unwrap_or_else(|e| panic!("{e}"));
        a.edit(
            "first\n",
            Principal::User {
                device: cfg_a.device,
            },
        )
        .unwrap_or_else(|e| panic!("{e}"));
        a.edit(
            "first\nsecond\n",
            Principal::User {
                device: cfg_a.device,
            },
        )
        .unwrap_or_else(|e| panic!("{e}"));
        let ops = ops_for(&store_a, &cfg_a.path);
        assert_eq!(ops.len(), 2);

        let (store_b, clock_b, cfg_b) = setup(dir.path(), 2);
        let mut b = NotesActor::open(cfg_b.clone(), Arc::clone(&store_b), clock_b)
            .unwrap_or_else(|e| panic!("{e}"));
        b.import_ops(ops).unwrap_or_else(|e| panic!("import: {e}"));
        assert_eq!(b.contents().0, b"first\nsecond\n");
        let on_disk = std::fs::read_to_string(&cfg_b.disk).unwrap_or_default();
        assert_eq!(on_disk, "first\nsecond\n");
        assert_eq!(
            ops_for(&store_b, &cfg_b.path).len(),
            2,
            "the batch is in b's log too"
        );
    }
}
