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
use crate::notes_mirror::loro_peer;
use crate::write::write_atomic_if;
use std::sync::PoisonError;
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
        let fork_peer = fork_peer(loro_peer(self.cfg.device), held, disk);
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
            self.store_held(None)?;
        } else {
            log_write_held(&self.cfg.path);
            let held = Held {
                base: self.written.clone(),
                base_snapshot: self.written_snapshot.clone(),
            };
            self.store_held(Some(&held))?;
            self.held = Some(held);
        }
        Ok(())
    }

    /// Keeps a held base in the store's meta (`held_base/<file>`, as `pending_save.rs` does for
    /// a list), so a restart before the merge resumes it three-way instead of reading the held
    /// edits as deleted text; `None` clears it once a write lands. Skipped when nothing changes.
    fn store_held(&mut self, held: Option<&Held>) -> Result<(), ActorError> {
        if held.is_none() && !self.held_stored {
            return Ok(());
        }
        let bytes = held.map(encode_held).unwrap_or_default();
        let key = held_key(&self.cfg.path);
        let mut store = self.store.lock().unwrap_or_else(PoisonError::into_inner);
        store.meta_set(&key, &bytes)?;
        self.held_stored = held.is_some();
        Ok(())
    }

    /// At open, before the disk is compared: a base a stop left held. What we last wrote is then
    /// that base, not the projection, which holds the edits the disk never got.
    pub(super) fn restore_held(&mut self) -> Result<(), ActorError> {
        let key = held_key(&self.cfg.path);
        let stored = {
            let store = self.store.lock().unwrap_or_else(PoisonError::into_inner);
            store.meta_get(&key)?
        };
        if let Some(held) = stored.as_deref().and_then(decode_held) {
            self.written.clone_from(&held.base);
            self.written_snapshot.clone_from(&held.base_snapshot);
            self.held = Some(held);
            self.held_stored = true;
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

/// The fork's own Loro peer: its ops must never reuse an id this device's mirror already has
/// (a clock-made id collided in a test, and Loro then dropped part of the merge). Derived from
/// what is merged, so the same merge twice makes the same ops, which an import takes once.
/// `blake3::Hasher`: <https://docs.rs/blake3/latest/blake3/struct.Hasher.html>
fn fork_peer(own: u64, held: &Held, disk: &[u8]) -> u64 {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"txtodo notes merge fork");
    hasher.update(&held.base_snapshot);
    hasher.update(disk);
    let mut first = [0u8; 8];
    first.copy_from_slice(&hasher.finalize().as_bytes()[..8]);
    let peer = u64::from_le_bytes(first);
    if peer == own { peer ^ 1 } else { peer }
}

fn held_key(path: &txtodo_model::FilePath) -> String {
    format!("held_base/{path}")
}

/// The snapshot's length on its own line, the snapshot, then the base. Empty: nothing is held.
fn encode_held(held: &Held) -> Vec<u8> {
    let mut out = format!("{}\n", held.base_snapshot.len()).into_bytes();
    out.extend_from_slice(&held.base_snapshot);
    out.extend_from_slice(&held.base);
    out
}

/// `encode_held`'s inverse; `None` for empty or unreadable bytes (then nothing is held).
fn decode_held(bytes: &[u8]) -> Option<Held> {
    let end = bytes.iter().position(|b| *b == b'\n')?;
    let len: usize = std::str::from_utf8(&bytes[..end]).ok()?.parse().ok()?;
    let rest = bytes.get(end + 1..)?;
    let snapshot = rest.get(..len)?;
    Some(Held {
        base: rest.get(len..)?.to_vec(),
        base_snapshot: snapshot.to_vec(),
    })
}

fn log_write_held(path: &txtodo_model::FilePath) {
    tracing::info!(file = %path, "notes_write_held_for_save");
}

fn log_save_merged(path: &txtodo_model::FilePath) {
    tracing::info!(file = %path, "notes_save_merged");
}
