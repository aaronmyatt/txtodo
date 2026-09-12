//! The notes actor's Loro mirror of one `notes.md` (plan M5), the notes analogue of `mirror.rs`.
//! `NotesState` stays the byte-faithful truth for what is on disk; the mirror is a
//! `txtodo_crdt::NotesDoc` fed the same edits, so a merge engine with local history exists for
//! sync to export/import against. Unlike the task document's mirror, a remote import needs no
//! Loro-diff-to-ops translation: notes has no list/map structure to reconcile, so the daemon just
//! diffs the text before and after the import (`txtodo_core::diff_text`) to get one log entry that
//! reproduces the merge on replay.

use std::fmt;

use crate::notes_state::NotesState;
use txtodo_crdt::{NotesDoc, NotesDocError};
use txtodo_model::{FilePath, Op, OpKind};

/// Why the notes mirror could not follow an op or import. Logged and healed the same way the task
/// mirror is — never a client error.
#[derive(Debug)]
pub enum NotesMirrorError {
    /// An op this mirror does not recognise as a `NotesEdit` for its own path.
    WrongOp,
    /// The underlying Loro document refused the operation.
    Doc(NotesDocError),
}

impl fmt::Display for NotesMirrorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NotesMirrorError::WrongOp => write!(f, "op is not a NotesEdit for this document"),
            NotesMirrorError::Doc(e) => write!(f, "notes mirror: {e}"),
        }
    }
}

impl std::error::Error for NotesMirrorError {}

impl From<NotesDocError> for NotesMirrorError {
    fn from(e: NotesDocError) -> NotesMirrorError {
        NotesMirrorError::Doc(e)
    }
}

/// One `notes.md`'s Loro mirror.
pub struct NotesMirror {
    doc: NotesDoc,
    path: FilePath,
}

impl NotesMirror {
    /// A fresh mirror seeded with the state's text: a new lineage, for a first open with no
    /// snapshot yet (mirrors `Mirror::from_state`'s doc).
    pub fn from_state(state: &NotesState, peer: u64) -> Result<NotesMirror, NotesMirrorError> {
        let doc = NotesDoc::hydrate(state.text())?;
        doc.set_peer(peer)?;
        Ok(NotesMirror {
            doc,
            path: state.path().clone(),
        })
    }

    /// The mirror a previous run stored; the caller replays the ops committed since.
    pub fn from_snapshot(
        bytes: &[u8],
        path: &FilePath,
        peer: u64,
    ) -> Result<NotesMirror, NotesMirrorError> {
        let doc = NotesDoc::from_snapshot(bytes)?;
        doc.set_peer(peer)?;
        Ok(NotesMirror {
            doc,
            path: path.clone(),
        })
    }

    /// Applies one local edit directly (we already know the exact edits — no need to derive them
    /// from a diff, unlike an import).
    pub fn flush(&mut self, ops: &[Op]) -> Result<(), NotesMirrorError> {
        for op in ops {
            let OpKind::NotesEdit { file, edits } = &op.kind else {
                return Err(NotesMirrorError::WrongOp);
            };
            if *file != self.path {
                return Err(NotesMirrorError::WrongOp);
            }
            self.doc.apply_edits(edits)?;
        }
        Ok(())
    }

    /// Everything the mirror holds, to persist.
    pub fn snapshot(&self) -> Result<Vec<u8>, NotesMirrorError> {
        Ok(self.doc.snapshot()?)
    }

    /// The mirror's version, for a peer to export updates since.
    pub fn version(&self) -> Vec<u8> {
        self.doc.version_bytes()
    }

    /// The updates a peer at `since` is missing.
    pub fn export_since(&self, since: &[u8]) -> Result<Vec<u8>, NotesMirrorError> {
        Ok(self.doc.export_updates_since(since)?)
    }

    /// The mirror's current text (the last-resort truth, and what an import merges into).
    pub fn text(&self) -> String {
        self.doc.content()
    }

    /// Merges a peer's updates; returns the text before and after so the caller can diff one log
    /// entry that reproduces the merge (`txtodo_core::diff_text(before, after)`).
    pub fn import(&mut self, bytes: &[u8]) -> Result<(String, String), NotesMirrorError> {
        let before = self.doc.content();
        self.doc.import(bytes)?;
        let after = self.doc.content();
        Ok((before, after))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use txtodo_model::{DeviceId, Hlc, OpId, Principal, TextEdit, Ulid};

    fn path() -> FilePath {
        FilePath::new("q4/abc/notes.md").unwrap_or_else(|e| panic!("{e}"))
    }

    fn edit_op(edits: Vec<TextEdit>) -> Op {
        let device = DeviceId::new(Ulid::from_u128(1));
        Op {
            id: OpId::new(Ulid::from_u128(2)),
            hlc: Hlc::zero(device),
            principal: Principal::User { device },
            file: path(),
            kind: OpKind::NotesEdit {
                file: path(),
                edits,
            },
        }
    }

    #[test]
    fn flush_applies_local_edits_and_snapshot_round_trips() {
        let state = NotesState::from_bytes(path(), b"hello").unwrap_or_else(|e| panic!("{e}"));
        let mut mirror = NotesMirror::from_state(&state, 1).unwrap_or_else(|e| panic!("{e}"));
        mirror
            .flush(&[edit_op(vec![TextEdit::Insert {
                at: 5,
                text: ", world".into(),
            }])])
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(mirror.text(), "hello, world");
        let snap = mirror.snapshot().unwrap_or_else(|e| panic!("{e}"));
        let restored =
            NotesMirror::from_snapshot(&snap, &path(), 1).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(restored.text(), "hello, world");
    }

    #[test]
    fn import_reports_text_before_and_after() {
        let a_state = NotesState::from_bytes(path(), b"abcdef").unwrap_or_else(|e| panic!("{e}"));
        let mut a = NotesMirror::from_state(&a_state, 1).unwrap_or_else(|e| panic!("{e}"));
        let snap = a.snapshot().unwrap_or_else(|e| panic!("{e}"));
        let mut b = NotesMirror::from_snapshot(&snap, &path(), 2).unwrap_or_else(|e| panic!("{e}"));
        a.flush(&[edit_op(vec![TextEdit::Insert {
            at: 0,
            text: "X-".into(),
        }])])
        .unwrap_or_else(|e| panic!("{e}"));
        let since = b.version();
        let updates = a.export_since(&since).unwrap_or_else(|e| panic!("{e}"));
        let (before, after) = b.import(&updates).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(before, "abcdef");
        assert_eq!(after, "X-abcdef");
    }
}
