//! One `notes.md`'s owner (plan M5): the notes analogue of `FileActor`, deliberately not a
//! `FileActor`/`DocState` (see `notes_state.rs`'s module doc — that reintroduces the id-stamping-
//! into-prose bug `workspace_tests::notes_md_is_left_alone` guards against). Same discipline as
//! every other document in this crate: store first (`Store::commit_change`), then rename
//! (`write::write_atomic`, temp + fsync + rename), and a hash comparison at open time to tell our
//! own projection from a foreign edit.
//!
//! No mailbox: a save is absorbed on its watcher event, at open and before each write
//! (`absorb_disk`; notes-watch, editor-save-lost). One writer: `notes_registry.rs`'s
//! `Arc<Mutex<NotesActor>>` per path: every call for one `ref:` directory takes the same lock.

use std::path::PathBuf;
use std::sync::Arc;

use crate::actor::{SharedStore, hash_of};
use crate::clock::Clock;
use crate::expected::Hash;
use crate::handle::{ActorError, Applied};
use crate::notes_mirror::NotesMirror;
use crate::notes_repair::{WaitingEdits, log_skipped, repair_edits, replay_leniently};
use crate::notes_state::NotesState;
use std::sync::PoisonError;
use txtodo_model::{DeviceId, FilePath, Hlc, Op, OpId, OpKind, Principal, TextEdit};
use txtodo_store::{Projection, Seq, SeqRange};

// A save the actor has not merged yet is held, never written over (task notes-watch).
#[path = "notes_held.rs"]
mod held;
// Restoring the Loro mirror at open, split out for the file budget.
#[path = "notes_actor_load.rs"]
mod load;
#[path = "notes_clock.rs"]
mod stamps;

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
    /// What we last put on disk, and the mirror snapshot that matched it (`notes_held.rs`).
    written: Vec<u8>,
    written_snapshot: Vec<u8>,
    held: Option<held::Held>,
    /// The disk's hash while a commit merges it, so its write may replace those bytes.
    merging: Option<Hash>,
    wrote_last: bool,
    held_stored: bool,
    /// Every op this document's log holds, order-free (`op_set_hash.rs`).
    op_set: crate::op_set_hash::OpSetHash,
    /// Peer edits that did not fit yet (`notes_repair.rs`).
    waiting: WaitingEdits,
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
        // Before `restore_held`: what it, `absorb_disk_text` and `repair_log` commit folds in on top.
        let op_set = {
            let guard = store.lock().unwrap_or_else(PoisonError::into_inner);
            crate::op_set_hash::OpSetHash::of_file(&guard, &cfg.path)?
        };
        // Text with no projection behind it came from a peer's mirror snapshot, not this log.
        let own_text = projection.is_some();
        let (mirror, state_bytes) = Self::load(&cfg, &store, projection, mirror_snapshot)?;
        let state = NotesState::from_bytes(cfg.path.clone(), &state_bytes)?;
        let bytes = state.to_bytes();
        let hash = hash_of(&bytes);
        let written_snapshot = mirror.snapshot().map_err(mirror_err)?;
        let mut actor = NotesActor {
            hlc: Hlc::zero(cfg.device),
            state,
            mirror,
            written: bytes.clone(),
            projection: bytes,
            hash,
            cfg,
            store,
            clock,
            written_snapshot,
            held: None,
            merging: None,
            wrote_last: false,
            held_stored: false,
            op_set,
            waiting: WaitingEdits::default(),
        };
        actor.adopt_logged_clock()?;
        actor.restore_held()?;
        actor.absorb_disk_text(&disk_bytes)?;
        if own_text {
            actor.repair_log()?;
        }
        Ok(actor)
    }

    /// Commits one `NotesEdit` taking "this file's log replayed from empty" to the current text,
    /// when the two differ (task notes-no-base, `notes_repair.rs`). A notes.md opened before
    /// v0.0.8 had its bytes adopted with no op, so a fresh peer replaying the log stopped at an
    /// op aimed past the end of an empty text. The text here does not change, only the log: the
    /// mirror already holds the text, so it is not flushed, and its snapshot moves to the new seq
    /// so a restart does not replay the op into it. Skipped when the replay was cut off.
    fn repair_log(&mut self) -> Result<(), ActorError> {
        let replayed = {
            let guard = self.store.lock().unwrap_or_else(PoisonError::into_inner);
            replay_leniently(&guard, &self.cfg.path, None)?
        };
        if !replayed.complete {
            return Ok(());
        }
        self.waiting = replayed.waiting.clone();
        let Some(edits) = repair_edits(&replayed.state, &self.state) else {
            // The log rebuilds this text: take its edit history too (ADR 0034).
            self.state.adopt_history(&replayed.state);
            return Ok(());
        };
        tracing::warn!(
            file = %self.cfg.path,
            log_chars = replayed.state.text().chars().count(),
            file_chars = self.state.text().chars().count(),
            "notes_log_repaired"
        );
        let hlc = self.tick()?;
        let device = self.cfg.device;
        let op = self.stamped(edits, hlc, Principal::External { device });
        let next = self.state.clone();
        let range = self.land(&[op], &next)?;
        self.persist_mirror(range)
    }

    /// Every op this document's log holds, order-free (task sync-divergence-check).
    pub fn op_set(&self) -> crate::op_set_hash::OpSetHash {
        self.op_set
    }

    /// Whether peer edits wait for one they build on: the text is about to change.
    pub(crate) fn has_waiting(&self) -> bool {
        !self.waiting.is_empty()
    }

    /// Current bytes and hash.
    pub fn contents(&self) -> (Vec<u8>, Hash) {
        (self.projection.clone(), self.hash)
    }

    /// Applies a whole-document replacement as one `NotesEdit` op: diffs it against the current
    /// text (`txtodo_core::diff_text`, the same char-level convention `EditText` uses), so
    /// concurrent edits from two devices still merge character-wise once both land in the mirror.
    pub fn edit(&mut self, new_text: &str, principal: Principal) -> Result<Applied, ActorError> {
        self.merge_if_held()?;
        self.edit_text(new_text, principal)
    }

    fn edit_text(&mut self, new_text: &str, principal: Principal) -> Result<Applied, ActorError> {
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
        self.merge_if_held()?;
        self.import_updates_as(updates, Principal::User { device: peer })
    }

    fn import_updates_as(
        &mut self,
        updates: &[u8],
        principal: Principal,
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
        let op = self.stamped(edits, hlc, principal);
        let mut next = self.state.clone();
        next.apply(&op)?;
        self.commit_without_mirror_flush(vec![op], next)?;
        Ok(Applied {
            applied: 1,
            hash: self.hash,
            hlc,
        })
    }

    /// Applies a peer's `NotesEdit` ops (task notes-sync), in the order they arrived. An op that
    /// does not fit the text is skipped with a `sync_op_skipped` warn (task notes-no-base,
    /// `notes_repair.rs`): refusing it made the peer resend it every 10 s, and every later op
    /// from that device waited behind it. Every op still lands in the log, so heads stay dense
    /// and the batch is acked; only the applied ones reach the Loro mirror. Only a store or disk
    /// failure refuses the batch.
    pub fn import_ops(&mut self, ops: Vec<Op>) -> Result<(), ActorError> {
        if ops.is_empty() {
            return Ok(());
        }
        self.merge_if_held()?;
        self.observe_peer_stamps(&ops);
        let mut next = self.state.clone();
        let mut waiting = self.waiting.clone();
        for (op, e) in waiting.apply(&mut next, &ops) {
            log_skipped(&op, &e);
        }
        let range = self.land(&ops, &next)?;
        self.waiting = waiting;
        // The mirror takes the state's text, not the batch's splices: an op that waited lands
        // later, and an older one the state keeps but cannot place (ADR 0034's rebuild skips it)
        // would not fit the mirror either.
        self.align_mirror()?;
        self.persist_mirror(range)
    }

    /// Brings the mirror to the state's text after an import: one edit, logged nowhere. The state
    /// orders text by stamp (ADR 0034) and keeps edits waiting (`notes_repair.rs`), so the batch's
    /// splices in arrival order would not give its text. The mirror never decides bytes, and
    /// `persist_mirror` keeps it aligned.
    fn align_mirror(&mut self) -> Result<(), ActorError> {
        let mirror_text = self.mirror.text();
        if mirror_text == self.state.text() {
            return Ok(());
        }
        let edits: Vec<TextEdit> = txtodo_core::diff_text(&mirror_text, self.state.text())
            .into_iter()
            .map(TextEdit::from)
            .collect();
        let device = self.cfg.device;
        let fix = self.stamped(edits, self.hlc, Principal::External { device });
        self.mirror.flush(&[fix]).map_err(mirror_err)
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
        self.op_set.add_ops(ops);
        self.state = next.clone();
        self.projection = bytes;
        self.hash = new_hash;
        self.write_or_hold()?;
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
        drop(store);
        self.note_written_snapshot(&snapshot);
        Ok(())
    }
}

fn mirror_err(e: crate::notes_mirror::NotesMirrorError) -> ActorError {
    ActorError::Mirror(e.to_string())
}
