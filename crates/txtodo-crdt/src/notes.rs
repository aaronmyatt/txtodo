//! A Loro text doc for one `notes.md` (plan M5, tasks/crdt-notes-doc): a single root `LoroText`,
//! no tasks map, no movable list — notes are prose, not task lines, so this is deliberately a
//! different, smaller document shape than [`crate::LoroDocument`] rather than a reuse of it (see
//! that type's module doc: the tasks map/movable list machinery has no meaning for prose).
//!
//! Edits replay through [`crate::to_loro::replay_edits`], the same dual-indexed `TextEdit` walk a
//! task's description uses, so two devices' concurrent edits to different parts of the text merge
//! character-wise the same way. Loro text API: <https://docs.rs/loro/latest/loro/struct.LoroText.html>.

use std::fmt;

use loro::{ExportMode, LoroDoc, LoroText, VersionVector};
use txtodo_model::TextEdit;

use crate::to_loro::replay_edits;

/// Root container name for the notes text; the only container this document ever holds.
const NOTES_TEXT_ROOT: &str = "notes";

/// Why a `NotesDoc` operation failed.
#[derive(Debug)]
pub enum NotesDocError {
    /// The underlying Loro operation failed.
    Loro(loro::LoroError),
    /// Encoding a snapshot or update export failed.
    Encode(loro::LoroEncodeError),
}

impl fmt::Display for NotesDocError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NotesDocError::Loro(e) => write!(f, "loro: {e}"),
            NotesDocError::Encode(e) => write!(f, "loro encode: {e}"),
        }
    }
}

impl std::error::Error for NotesDocError {}

impl From<loro::LoroError> for NotesDocError {
    fn from(e: loro::LoroError) -> NotesDocError {
        NotesDocError::Loro(e)
    }
}

impl From<loro::LoroEncodeError> for NotesDocError {
    fn from(e: loro::LoroEncodeError) -> NotesDocError {
        NotesDocError::Encode(e)
    }
}

/// The Loro-backed document behind one `notes.md`. In-memory only, like [`crate::LoroDocument`]:
/// `txtodo-store` is the durable truth and this is rebuilt from a snapshot plus replayed ops.
pub struct NotesDoc {
    doc: LoroDoc,
}

impl NotesDoc {
    /// Opens an empty document.
    pub fn open() -> NotesDoc {
        NotesDoc {
            doc: LoroDoc::new(),
        }
    }

    /// An empty document seeded with `initial` as one insert, for a fresh lineage (no snapshot
    /// exists yet — mirrors [`crate::LoroDocument::open`] plus one hydrate step).
    pub fn hydrate(initial: &str) -> Result<NotesDoc, NotesDocError> {
        let doc = NotesDoc::open();
        if !initial.is_empty() {
            doc.text().insert(0, initial)?;
            doc.doc.commit();
        }
        debug_assert_eq!(doc.content(), initial);
        Ok(doc)
    }

    /// Rebuilds a document from a snapshot written by [`NotesDoc::snapshot`].
    pub fn from_snapshot(bytes: &[u8]) -> Result<NotesDoc, NotesDocError> {
        Ok(NotesDoc {
            doc: LoroDoc::from_snapshot(bytes)?,
        })
    }

    /// Pins the Loro peer id, like [`crate::LoroDocument::set_peer`].
    pub fn set_peer(&self, peer: u64) -> Result<(), NotesDocError> {
        self.doc.set_peer_id(peer)?;
        debug_assert_eq!(self.doc.peer_id(), peer);
        Ok(())
    }

    fn text(&self) -> LoroText {
        self.doc.get_text(NOTES_TEXT_ROOT)
    }

    /// The current text.
    pub fn content(&self) -> String {
        self.text().to_string()
    }

    /// Replays a dual-indexed `TextEdit` stream (`txtodo_core::diff_text`'s convention) onto the
    /// text, one Loro commit. Concurrent edits from two devices land at different offsets and
    /// Loro's text CRDT transforms them on merge — this is the whole point of this document kind.
    pub fn apply_edits(&mut self, edits: &[TextEdit]) -> Result<(), NotesDocError> {
        let core_edits: Vec<txtodo_core::TextEdit> =
            edits.iter().cloned().map(Into::into).collect();
        let text = self.text();
        replay_edits(&text, &core_edits)?;
        self.doc.commit();
        Ok(())
    }

    /// Exports the current state as a full snapshot.
    pub fn snapshot(&self) -> Result<Vec<u8>, NotesDocError> {
        Ok(self.doc.export(ExportMode::Snapshot)?)
    }

    /// This document's version vector, opaque, for a peer to export updates since.
    pub fn version_bytes(&self) -> Vec<u8> {
        let bytes = self.doc.oplog_vv().encode();
        debug_assert!(VersionVector::decode(&bytes).is_ok());
        bytes
    }

    /// The updates a peer at `since` (its own `version_bytes()`) is missing.
    pub fn export_updates_since(&self, since: &[u8]) -> Result<Vec<u8>, NotesDocError> {
        let vv = VersionVector::decode(since)?;
        Ok(self.doc.export(ExportMode::updates(&vv))?)
    }

    /// Merges a peer's updates. Loro resolves the merge internally; the caller derives a local
    /// log entry from the text before and after, same as `txtodo_crdt::from_batch` derives ops for
    /// a task document — see `txtodo-daemon`'s `notes_mirror.rs`.
    pub fn import(&mut self, bytes: &[u8]) -> Result<(), NotesDocError> {
        self.doc.import(bytes)?;
        Ok(())
    }
}

impl Default for NotesDoc {
    fn default() -> NotesDoc {
        NotesDoc::open()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hydrate_round_trips_and_edits_apply() {
        let mut doc = NotesDoc::hydrate("hello world").unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(doc.content(), "hello world");
        doc.apply_edits(&[TextEdit::Insert {
            at: 5,
            text: ",".into(),
        }])
        .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(doc.content(), "hello, world");
    }

    #[test]
    fn snapshot_round_trips() {
        let doc = NotesDoc::hydrate("abc").unwrap_or_else(|e| panic!("{e}"));
        let bytes = doc.snapshot().unwrap_or_else(|e| panic!("{e}"));
        let restored = NotesDoc::from_snapshot(&bytes).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(restored.content(), "abc");
    }

    #[test]
    fn two_devices_forked_from_one_snapshot_merge_concurrent_edits() {
        let mut a = NotesDoc::hydrate("abcdef").unwrap_or_else(|e| panic!("{e}"));
        a.set_peer(1).unwrap_or_else(|e| panic!("{e}"));
        let snap = a.snapshot().unwrap_or_else(|e| panic!("{e}"));
        let mut b = NotesDoc::from_snapshot(&snap).unwrap_or_else(|e| panic!("{e}"));
        b.set_peer(2).unwrap_or_else(|e| panic!("{e}"));
        // A prepends, B appends — different offsets, both should survive the merge.
        a.apply_edits(&[TextEdit::Insert {
            at: 0,
            text: "X-".into(),
        }])
        .unwrap_or_else(|e| panic!("{e}"));
        b.apply_edits(&[TextEdit::Insert {
            at: 6,
            text: "-Y".into(),
        }])
        .unwrap_or_else(|e| panic!("{e}"));
        let since = b.version_bytes();
        let updates = a
            .export_updates_since(&since)
            .unwrap_or_else(|e| panic!("{e}"));
        b.import(&updates).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(b.content(), "X-abcdef-Y");
    }
}
