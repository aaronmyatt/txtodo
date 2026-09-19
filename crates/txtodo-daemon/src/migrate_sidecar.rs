//! Tagged → Sidecar migration of one document (`tasks/sidecar-migrate-tagged`, ADR 0019).
//!
//! One commit per document: an `EditText` op per tagged line that drops its own `id:` tag (the
//! same op a user edit makes, so history, blame, undo and the Loro mirror all see an ordinary
//! edit), the state rebuilt in `Sidecar` mode, and the fingerprints landed in the same store
//! transaction (`actor_mirror::fingerprints_for`) under the ULIDs the tags used to carry. Task
//! identity therefore survives; only the text changes.

use crate::actor::{Commit, CommitTail, FileActor};
use crate::handle::{ActorError, ActorHandle, ActorMsg};
use crate::id_strip::strip_own_id;
use crate::reconcile::change_ops;
use crate::state::{DocState, Entry};
use std::collections::HashSet;
use txtodo_core::parse_file;
use txtodo_model::{IdentityMode, OpKind, Principal, TaskId};

/// What migrating one document did (or, for a dry run, would do).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Migrated {
    /// Task lines in the document.
    pub tasks: usize,
    /// Task lines whose own `id:` tag was (or would be) removed.
    pub stripped: usize,
    /// Task lines that shared their id with an earlier line in the same document and were given a
    /// fresh one. Sidecar identity is one row per `(file, task)`, so two lines cannot keep one id.
    pub renumbered: usize,
}

impl ActorHandle {
    /// Migrates this document to sidecar identity; `dry_run` only counts.
    pub async fn migrate_to_sidecar(&self, dry_run: bool) -> Result<Migrated, ActorError> {
        self.ask(|reply| ActorMsg::MigrateToSidecar { dry_run, reply })
            .await?
    }
}

impl FileActor {
    /// Idempotent: a document that already has fingerprint rows under Sidecar is done and is left
    /// alone, so re-running after an interrupted migration only finishes the documents still
    /// tagged.
    ///
    /// A document where two lines carry the same `id:` (a copy-pasted tag; the Tagged state cannot
    /// address the second one, every op by that id lands on the first) keeps the first line's
    /// identity and gives each later one a fresh id. Ops cannot express that, so such a document
    /// commits its final state directly, pins a snapshot for replay and rebuilds its Loro mirror.
    pub(crate) fn on_migrate_to_sidecar(&mut self, dry_run: bool) -> Result<Migrated, ActorError> {
        if self.is_migrated()? {
            let tasks = self.task_count();
            return Ok(Migrated {
                tasks,
                ..Migrated::default()
            });
        }
        let plain = self.entry_ids();
        let (ids, dups) = self.renumber_duplicates(&plain);
        let (mut report, kinds) = self.scan(&dups);
        report.renumbered = dups.len();
        if dry_run {
            return Ok(report);
        }
        if kinds.is_empty() {
            // Nothing to strip (an empty document): only the mode changes, so a task added later
            // gets no `id:` tag from a state that still says Tagged.
            self.state = self.as_sidecar(&self.projection.clone(), &plain)?;
            self.cfg.identity_mode = IdentityMode::Sidecar;
            return Ok(report);
        }
        self.commit_migration(kinds, &plain, &ids, &dups)?;
        Ok(report)
    }

    /// Migrated already: Sidecar with fingerprint rows. Not "no own tags left" — a line that
    /// mentions another `id:<ULID>` in its prose reads as tagged again once its real tag is gone,
    /// and must be left alone.
    fn is_migrated(&self) -> Result<bool, ActorError> {
        Ok(self.cfg.identity_mode == IdentityMode::Sidecar
            && !self
                .lock_store()
                .live_fingerprints(&self.cfg.path)?
                .is_empty())
    }

    fn task_count(&self) -> usize {
        (0..self.state.len())
            .filter(|&i| matches!(self.state.entry_at(i), Some(Entry::Task { .. })))
            .count()
    }

    /// Counts the task lines and the tags to strip, and the ops that strip every tag whose line is
    /// not a repeat (`dups`): those are rewritten in the final state instead.
    fn scan(&self, dups: &[usize]) -> (Migrated, Vec<OpKind>) {
        let mut report = Migrated::default();
        let mut kinds = Vec::new();
        for i in 0..self.state.len() {
            let Some(Entry::Task { id, line }) = self.state.entry_at(i) else {
                continue;
            };
            report.tasks += 1;
            if let Some((_, stripped)) = strip_own_id(&line) {
                report.stripped += 1;
                if !dups.contains(&i) {
                    kinds.extend(change_ops(&line, &stripped, id));
                }
            }
        }
        (report, kinds)
    }

    /// Lands the migration as one commit (see the module doc).
    fn commit_migration(
        &mut self,
        kinds: Vec<OpKind>,
        plain: &[Option<TaskId>],
        ids: &[Option<TaskId>],
        dups: &[usize],
    ) -> Result<(), ActorError> {
        let device = self.cfg.device;
        let ops = self.stamp(kinds, &Principal::User { device })?;
        // The edits apply to a Sidecar copy of the state: a Tagged one refuses any edit that
        // removes the tag (`IdMismatch`), which is the whole point here.
        let mut edited = self.as_sidecar(&self.projection.clone(), plain)?;
        for op in &ops {
            edited.apply(op)?;
        }
        let mut file = parse_file(&edited.to_bytes());
        for &i in dups {
            if let (Some(Entry::Task { line, .. }), Some(slot)) =
                (self.state.entry_at(i), file.lines.get_mut(i))
                && let Some((_, stripped)) = strip_own_id(&line)
            {
                *slot = stripped;
            }
        }
        let bytes = file.to_bytes();
        let next = self.as_sidecar(&bytes, ids)?;
        let write = bytes != self.projection;
        // `commit` lands fingerprints only while the config says Sidecar; put it back on failure so
        // a retry sees the mode this document really is in.
        let before = std::mem::replace(&mut self.cfg.identity_mode, IdentityMode::Sidecar);
        let plan = Commit {
            ops,
            next,
            bytes,
            write,
            snapshot: false,
            tail: CommitTail {
                flush: dups.is_empty(),
                ..CommitTail::default()
            },
        };
        if let Err(e) = self.commit(plan) {
            self.cfg.identity_mode = before;
            return Err(e);
        }
        if !dups.is_empty() {
            // The ops do not add up to the state, and the old mirror held the repeated ids it
            // cannot converge from: pin a snapshot for replay and start a fresh mirror lineage.
            self.resync_mirror();
            self.maybe_snapshot(None, true)?;
        }
        Ok(())
    }

    /// Every entry's id as the current state has it (`None` for a blank), one per line.
    fn entry_ids(&self) -> Vec<Option<TaskId>> {
        (0..self.state.len())
            .map(|i| self.state.entry_at(i).and_then(|e| e.id()))
            .collect()
    }

    /// `plain` with every repeat of an id replaced by a freshly minted one, plus the indices of the
    /// lines that got one.
    fn renumber_duplicates(&self, plain: &[Option<TaskId>]) -> (Vec<Option<TaskId>>, Vec<usize>) {
        let mut seen = HashSet::new();
        let mut dups = Vec::new();
        let ids = plain
            .iter()
            .enumerate()
            .map(|(i, id)| match id {
                Some(id) if !seen.insert(*id) => {
                    dups.push(i);
                    Some(TaskId::new(self.clock.new_ulid()))
                }
                other => *other,
            })
            .collect();
        (ids, dups)
    }

    /// `bytes` as a Sidecar `DocState` with `ids[i]` for line `i` — the ids line up one for one
    /// with this state's entries, like `reconcile_against`.
    fn as_sidecar(&self, bytes: &[u8], ids: &[Option<TaskId>]) -> Result<DocState, ActorError> {
        Ok(DocState::from_file(
            self.cfg.path.clone(),
            &parse_file(bytes),
            ids,
            IdentityMode::Sidecar,
        )?)
    }
}
