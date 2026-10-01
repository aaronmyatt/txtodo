//! A save on disk the notes actor has not merged yet is never written over (task notes-watch,
//! `tasks/notes-watch/notes.md`), the `notes.md` twin of `pending_save.rs`. A child of
//! `notes_actor.rs`, so it reads the actor's fields.
//!
//! Every write checks the file right before its rename (`write::write_atomic_if`): unless it is
//! what we last wrote, the bytes this commit merges, or the new bytes themselves, the rename is
//! dropped. The commit still lands in the store, the state and the mirror; what we last wrote
//! stays the merge base, with the mirror snapshot that matched it. The watcher's event then
//! merges three-way: the editor's change (base to disk) is applied on a fork of the mirror at the
//! base and imported, so Loro places it on top of whatever peers wrote meanwhile. Absorbing the
//! disk before every write instead read a save before its debounce and could catch it half-way.

use super::{NotesActor, mirror_err};
use crate::actor::hash_of;
use crate::handle::ActorError;
use crate::notes_mirror::NotesMirror;
use crate::write::write_atomic_if;
use txtodo_model::{Principal, TextEdit};

/// What we last wrote while a write is held: the three-way merge's common ancestor.
pub(super) struct Held {
    /// The bytes on disk before the editor's save.
    base: Vec<u8>,
    /// The mirror as it was when `base` was written: the merge forks here.
    base_snapshot: Vec<u8>,
}

impl NotesActor {
    /// Text on disk that is not ours, committed as this device's `External` edit and never
    /// written over: at open, on the watcher's debounced event (`watch_task.rs`), and before a
    /// write while one is held. With a held write it merges three-way; otherwise the disk is
    /// simply the editor's new text.
    pub(crate) fn absorb_disk(&mut self) -> Result<(), ActorError> {
        let disk = std::fs::read(&self.cfg.disk).unwrap_or_default();
        self.absorb_disk_text(&disk)
    }

    pub(super) fn absorb_disk_text(&mut self, disk: &[u8]) -> Result<(), ActorError> {
        let disk_hash = hash_of(disk);
        if disk_hash == hash_of(&self.written) || disk_hash == self.hash {
            // Nothing foreign there (any more): a held write goes out now.
            if self.held.take().is_some() {
                self.write_now()?;
            }
            return Ok(());
        }
        self.merging = Some(disk_hash);
        self.wrote_last = false;
        let device = self.cfg.device;
        let outcome = match self.held.take() {
            Some(held) => self
                .merge_held(&held, disk)
                .inspect_err(|_| self.held = Some(held)),
            None => {
                let text = String::from_utf8_lossy(disk).into_owned();
                self.edit_text(&text, Principal::External { device })
                    .map(|_| ())
            }
        };
        // A merge that changed nothing committed nothing, so nothing was written yet.
        let outcome = outcome.and_then(|()| match self.wrote_last {
            true => Ok(()),
            false => self.write_now(),
        });
        self.merging = None;
        outcome
    }

    /// Writes the projection now, outside a commit (a held write released, a merge that changed
    /// nothing), and takes the mirror's snapshot as the next merge base when it lands.
    fn write_now(&mut self) -> Result<(), ActorError> {
        self.write_or_hold()?;
        if self.wrote_last {
            let snapshot = self.mirror.snapshot().map_err(mirror_err)?;
            self.note_written_snapshot(&snapshot);
        }
        Ok(())
    }

    /// Before a write: a held save is merged first, so the write lands on the merged text.
    pub(super) fn merge_if_held(&mut self) -> Result<(), ActorError> {
        if self.held.is_some() {
            self.absorb_disk()?;
        }
        Ok(())
    }

    /// The editor's change (`held.base` to `disk`) made on a fork of the mirror at the base, then
    /// imported: Loro merges it with every edit made since, and the import's diff is one op.
    fn merge_held(&mut self, held: &Held, disk: &[u8]) -> Result<(), ActorError> {
        let base = String::from_utf8_lossy(&held.base);
        let saved = String::from_utf8_lossy(disk);
        let edits: Vec<TextEdit> = txtodo_core::diff_text(&base, &saved)
            .into_iter()
            .map(TextEdit::from)
            .collect();
        // Its own Loro peer: the fork's ops must never reuse a counter this device's mirror has.
        let fork_peer = (self.clock.new_ulid().to_u128() & u128::from(u64::MAX)) as u64;
        let mut fork = NotesMirror::from_snapshot(&held.base_snapshot, &self.cfg.path, fork_peer)
            .map_err(mirror_err)?;
        let since = fork.version();
        let hlc = self.hlc;
        let device = self.cfg.device;
        let change = self.stamped(edits, hlc, Principal::External { device });
        fork.flush(&[change]).map_err(mirror_err)?;
        let updates = fork.export_since(&since).map_err(mirror_err)?;
        log_save_merged(&self.cfg.path);
        self.import_updates_as(&updates, Principal::External { device })
            .map(|_| ())
    }

    /// The write of `land`: the new projection replaces the file only if, right before the
    /// rename, the file is still ours; otherwise the save there is held, based on what we last
    /// wrote. While a write is held and this commit is not its merge, nothing is written.
    pub(super) fn write_or_hold(&mut self) -> Result<(), ActorError> {
        self.wrote_last = false;
        if self.held.is_some() && self.merging.is_none() {
            return Ok(());
        }
        let ours = [Some(hash_of(&self.written)), self.merging, Some(self.hash)];
        let disk = self.cfg.disk.clone();
        let wrote = write_atomic_if(&self.cfg.disk, &self.projection, || {
            let on_disk = std::fs::read(&disk).unwrap_or_default();
            Ok(ours.contains(&Some(hash_of(&on_disk))))
        })?;
        if wrote {
            self.written.clone_from(&self.projection);
            self.wrote_last = true;
        } else {
            log_write_held(&self.cfg.path);
            self.held = Some(Held {
                base: self.written.clone(),
                base_snapshot: self.written_snapshot.clone(),
            });
        }
        Ok(())
    }

    /// After a commit's mirror snapshot: if that commit wrote the file, the snapshot is what a
    /// later merge forks from.
    pub(super) fn note_written_snapshot(&mut self, snapshot: &[u8]) {
        if self.wrote_last {
            self.written_snapshot = snapshot.to_vec();
        }
    }
}

fn log_write_held(path: &txtodo_model::FilePath) {
    tracing::info!(file = %path, "notes_write_held_for_save");
}

fn log_save_merged(path: &txtodo_model::FilePath) {
    tracing::info!(file = %path, "notes_save_merged");
}
