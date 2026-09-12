//! In-memory state of one `notes.md`: the exact file content as one string, no lines, no ids, no
//! blanks-as-entries (tasks/crdt-notes-doc: "a notes document is a single LoroText plus the file's
//! byte-fidelity bits"). Deliberately not a `DocState`: this crate must never interpret notes.md
//! prose as task lines (`workspace_tests::notes_md_is_left_alone`).
//!
//! Byte fidelity here is trivial rather than reconstructed: `text` *is* the file's UTF-8 content,
//! BOM included as its own leading character (`\u{FEFF}`, exactly what the three BOM bytes decode
//! to) and every line ending kept as literal `\n`/`\r\n` bytes inside the string. There is no
//! line-splitting step to get wrong — `to_bytes` is `text.as_bytes()`, full stop. An earlier
//! version of this module borrowed `DocState`'s per-line split/rejoin (BOM, dominant ending,
//! trailing-newline as separate fields) and it produced a real bug: reassembling lines with `\n`
//! joins added a newline `state.text()` never had, so replaying the client's own unedited text back
//! against it minted a spurious edit. A prose document has no "lines" to preserve fidelity between
//! in the first place, so there is nothing to reconstruct.

use std::fmt;
use txtodo_model::{FilePath, Op, OpKind, TextEdit};

use crate::textedit::{TextEditError, apply_notes_edits};

/// Largest `notes.md` this crate accepts (prose has no line cap, but "as big as the disk" is not
/// a bound). Generous: about 8x this crate's own per-file projection ceiling headroom for prose.
pub const MAX_NOTES_BYTES: usize = 8 * 1024 * 1024;

/// Why a notes op or byte buffer could not be applied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotesStateError {
    /// The bytes are not valid UTF-8 (a `LoroText` is inherently Unicode).
    NotUtf8,
    /// The document would exceed `MAX_NOTES_BYTES`.
    TooManyBytes(usize),
    /// A text edit did not fit the current text.
    Text(TextEditError),
    /// The op kind is not a `NotesEdit` (a daemon bug: only `NotesEdit` ever reaches this type).
    Unsupported(&'static str),
    /// The op named a different document than this state's path (a daemon bug).
    WrongFile(FilePath),
}

impl fmt::Display for NotesStateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NotesStateError::NotUtf8 => write!(f, "notes.md is not valid UTF-8"),
            NotesStateError::TooManyBytes(n) => {
                write!(f, "notes.md would be {n} bytes, max {MAX_NOTES_BYTES}")
            }
            NotesStateError::Text(e) => write!(f, "{e}"),
            NotesStateError::Unsupported(what) => write!(f, "{what} is not a notes.md op"),
            NotesStateError::WrongFile(p) => write!(f, "op is for {p}, not this document"),
        }
    }
}

impl std::error::Error for NotesStateError {}

/// One `notes.md`'s state: the exact file content as one string.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NotesState {
    path: FilePath,
    text: String,
}

impl NotesState {
    /// An empty document at `path` (no file on disk yet, or the ref: directory was just created).
    pub fn empty(path: FilePath) -> NotesState {
        NotesState {
            path,
            text: String::new(),
        }
    }

    /// Builds the state from raw file bytes: the content, verbatim, as UTF-8 text.
    pub fn from_bytes(path: FilePath, bytes: &[u8]) -> Result<NotesState, NotesStateError> {
        if bytes.len() > MAX_NOTES_BYTES {
            return Err(NotesStateError::TooManyBytes(bytes.len()));
        }
        let text = std::str::from_utf8(bytes)
            .map_err(|_| NotesStateError::NotUtf8)?
            .to_owned();
        Ok(NotesState { path, text })
    }

    /// The document's path.
    pub fn path(&self) -> &FilePath {
        &self.path
    }

    /// The current prose.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The file as bytes: exactly `text`'s UTF-8 encoding, nothing added or removed.
    pub fn to_bytes(&self) -> Vec<u8> {
        self.text.clone().into_bytes()
    }

    /// Applies one `NotesEdit` op. On `Err` the state is unchanged.
    pub fn apply(&mut self, op: &Op) -> Result<(), NotesStateError> {
        let OpKind::NotesEdit { file, edits } = &op.kind else {
            return Err(NotesStateError::Unsupported(op_kind_name(&op.kind)));
        };
        if *file != self.path || op.file != self.path {
            return Err(NotesStateError::WrongFile(file.clone()));
        }
        self.apply_edits(edits)
    }

    /// Applies a bare edit stream, independent of any `Op` wrapper (undo/checkout replay).
    pub fn apply_edits(&mut self, edits: &[TextEdit]) -> Result<(), NotesStateError> {
        let next = apply_notes_edits(&self.text, edits).map_err(NotesStateError::Text)?;
        if next.len() > MAX_NOTES_BYTES {
            return Err(NotesStateError::TooManyBytes(next.len()));
        }
        self.text = next;
        Ok(())
    }
}

fn op_kind_name(kind: &OpKind) -> &'static str {
    match kind {
        OpKind::Insert { .. } => "Insert",
        OpKind::SetField { .. } => "SetField",
        OpKind::EditText { .. } => "EditText",
        OpKind::Move { .. } => "Move",
        OpKind::NotesEdit { .. } => "NotesEdit",
        OpKind::BlankInsert { .. } => "BlankInsert",
        OpKind::BlankRemove { .. } => "BlankRemove",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use txtodo_model::{DeviceId, Hlc, OpId, Principal, Ulid};

    fn path() -> FilePath {
        FilePath::new("q4/abc/notes.md").unwrap_or_else(|e| panic!("{e}"))
    }

    fn op(edits: Vec<TextEdit>) -> Op {
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
    fn round_trips_bom_crlf_and_missing_trailing_newline() {
        let bytes = b"\xEF\xBB\xBFline one\r\nline two";
        let state = NotesState::from_bytes(path(), bytes).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(state.text(), "\u{FEFF}line one\r\nline two");
        assert_eq!(state.to_bytes(), bytes);
    }

    #[test]
    fn empty_bytes_round_trip_to_empty_bytes() {
        let state = NotesState::from_bytes(path(), b"").unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(state.text(), "");
        assert_eq!(state.to_bytes(), b"");
    }

    #[test]
    fn reapplying_the_same_text_back_is_a_true_no_op() {
        // The bug this module doc calls out: joining split lines with `\n` used to add a newline
        // `text()` never had, so this round trip minted a spurious edit.
        let bytes = b"one two three four five\n";
        let state = NotesState::from_bytes(path(), bytes).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(state.to_bytes(), bytes);
        let same = std::str::from_utf8(bytes).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(state.text(), same, "no hidden reformatting between reads");
    }

    #[test]
    fn apply_replaces_text_and_rejects_the_wrong_document() {
        let mut state =
            NotesState::from_bytes(path(), b"hello\n").unwrap_or_else(|e| panic!("{e}"));
        state
            .apply(&op(vec![TextEdit::Insert {
                at: 5,
                text: ", world".into(),
            }]))
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(state.text(), "hello, world\n");
        let other = FilePath::new("other/notes.md").unwrap_or_else(|e| panic!("{e}"));
        let device = DeviceId::new(Ulid::from_u128(1));
        let bad = Op {
            id: OpId::new(Ulid::from_u128(3)),
            hlc: Hlc::zero(device),
            principal: Principal::User { device },
            file: other.clone(),
            kind: OpKind::NotesEdit {
                file: other,
                edits: Vec::new(),
            },
        };
        assert!(matches!(
            state.apply(&bad),
            Err(NotesStateError::WrongFile(_))
        ));
    }
}
