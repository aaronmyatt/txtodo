//! In-memory state of one document: the ordered entries the actor owns, materialised to the exact
//! bytes on disk, mutated only through ops. M4 swaps the backing store for Loro behind this same
//! shape (plan M4), so nothing outside this module touches `entries`.

use crate::textedit::TextEditError;
use std::fmt;
use txtodo_core::{File, LineEnding, LineKind, OwnedLine};
use txtodo_model::{FilePath, OpKind, TaskId, Ulid};

/// Most lines one document may hold; a 10k-line workspace is the perf target, this is 100× that.
pub const MAX_LINES_PER_FILE: usize = 1_000_000;

/// One line of the document.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Entry {
    /// A task line with a valid `id:` tag.
    Task {
        /// Its id.
        id: TaskId,
        /// Its bytes.
        line: OwnedLine,
    },
    /// An empty line (design §2.2 rule 6: blanks are entries).
    Blank(OwnedLine),
}

impl Entry {
    /// The line bytes.
    pub fn line(&self) -> &OwnedLine {
        match self {
            Entry::Task { line, .. } | Entry::Blank(line) => line,
        }
    }
    /// The task id, for task entries.
    pub fn id(&self) -> Option<TaskId> {
        match self {
            Entry::Task { id, .. } => Some(*id),
            Entry::Blank(_) => None,
        }
    }
}

/// Why an op or a file could not be applied. An op that fails here is a daemon bug or a stale
/// client; the message says which task and what was attempted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StateError {
    /// A task line has no valid `id:` tag (line index).
    MissingId(usize),
    /// A line is not valid UTF-8 (line index).
    Opaque(usize),
    /// The op names a task this document does not hold.
    UnknownTask(TaskId),
    /// `Insert` line text does not carry the op's task id.
    IdMismatch(TaskId),
    /// A text edit did not fit the description.
    Text(TaskId, TextEditError),
    /// `BlankRemove` found no blank at that position.
    NoBlank(Option<TaskId>),
    /// The op kind is not handled on one device in M3 (cross-file move, notes, undelete, quirks).
    Unsupported(&'static str),
    /// The document would exceed `MAX_LINES_PER_FILE`.
    TooManyLines(usize),
}

impl fmt::Display for StateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StateError::MissingId(i) => write!(f, "line {} has no id: tag", i + 1),
            StateError::Opaque(i) => write!(f, "line {} is not UTF-8", i + 1),
            StateError::UnknownTask(t) => write!(f, "no task {t} in this document"),
            StateError::IdMismatch(t) => write!(f, "inserted line does not carry id {t}"),
            StateError::Text(t, e) => write!(f, "task {t}: {e}"),
            StateError::NoBlank(after) => write!(f, "no blank line after {after:?}"),
            StateError::Unsupported(what) => {
                write!(f, "{what} is not supported on one device (M3)")
            }
            StateError::TooManyLines(n) => {
                write!(f, "document would have {n} lines, max {MAX_LINES_PER_FILE}")
            }
        }
    }
}

impl std::error::Error for StateError {}

/// The document.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DocState {
    path: FilePath,
    entries: Vec<Entry>,
    bom: bool,
    ending: LineEnding,
    trailing_newline: bool,
}

impl DocState {
    /// Builds the state from a parsed file. Every task line must already carry an `id:` (the
    /// reconciler assigns ids before calling this).
    pub fn from_file(path: FilePath, file: &File) -> Result<DocState, StateError> {
        if file.lines.len() > MAX_LINES_PER_FILE {
            return Err(StateError::TooManyLines(file.lines.len()));
        }
        let mut entries = Vec::with_capacity(file.lines.len());
        for (i, line) in file.lines.iter().enumerate() {
            entries.push(entry_of(i, line)?);
        }
        debug_assert_eq!(entries.len(), file.lines.len());
        Ok(DocState {
            path,
            entries,
            bom: file.bom,
            ending: file.ending,
            trailing_newline: file.trailing_newline,
        })
    }

    /// The document's path.
    pub fn path(&self) -> &FilePath {
        &self.path
    }

    /// The entries in file order.
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// Position of a task, if present.
    pub fn index_of(&self, id: TaskId) -> Option<usize> {
        self.entries.iter().position(|e| e.id() == Some(id))
    }

    /// Replaces the entry at `i` (fields.rs rewrites lines in place).
    pub(crate) fn replace_entry(&mut self, i: usize, entry: Entry) {
        debug_assert!(i < self.entries.len(), "replace_entry index {i} in range");
        self.entries[i] = entry;
    }

    /// Removes the entry at `i`.
    pub(crate) fn remove_entry(&mut self, i: usize) -> Entry {
        debug_assert!(i < self.entries.len(), "remove_entry index {i} in range");
        self.entries.remove(i)
    }

    /// The file as bytes, byte-faithful to what `from_file` read plus the applied ops.
    pub fn to_bytes(&self) -> Vec<u8> {
        let file = File {
            lines: self.entries.iter().map(|e| e.line().clone()).collect(),
            bom: self.bom,
            ending: self.ending,
            trailing_newline: self.trailing_newline,
        };
        debug_assert_eq!(file.lines.len(), self.entries.len());
        file.to_bytes()
    }

    /// Applies one op. On `Err` the state is unchanged.
    pub fn apply(&mut self, kind: &OpKind) -> Result<(), StateError> {
        let before = self.entries.len();
        match kind {
            OpKind::Insert { task, after, line } => self.insert(*task, *after, line)?,
            OpKind::SetField { task, field, value } => {
                crate::fields::set_field(self, *task, *field, *value)?
            }
            OpKind::EditText { task, edits } => crate::fields::edit_text(self, *task, edits)?,
            OpKind::Move {
                task,
                after,
                to_file,
            } => self.move_task(*task, *after, to_file)?,
            OpKind::BlankInsert { after } => self.blank_insert(*after)?,
            OpKind::BlankRemove { after } => self.blank_remove(*after)?,
            OpKind::NotesEdit { .. } => return Err(StateError::Unsupported("NotesEdit")),
        }
        debug_assert!(self.entries.len() <= MAX_LINES_PER_FILE);
        debug_assert!(
            self.entries.len() + 1 >= before,
            "an op removes at most one entry"
        );
        Ok(())
    }

    fn position_after(&self, after: Option<TaskId>) -> Result<usize, StateError> {
        match after {
            None => Ok(0),
            Some(id) => self
                .index_of(id)
                .map(|i| i + 1)
                .ok_or(StateError::UnknownTask(id)),
        }
    }

    fn insert(
        &mut self,
        task: TaskId,
        after: Option<TaskId>,
        line: &str,
    ) -> Result<(), StateError> {
        if self.entries.len() + 1 > MAX_LINES_PER_FILE {
            return Err(StateError::TooManyLines(self.entries.len() + 1));
        }
        let at = self.position_after(after)?;
        let owned = OwnedLine::from_bytes(line.as_bytes().to_vec(), self.ending);
        let parsed_id = owned.parse().and_then(|l| match l.kind {
            LineKind::Task(t) => t.id(),
            LineKind::Blank => None,
        });
        if parsed_id != Some(task.ulid()) {
            return Err(StateError::IdMismatch(task));
        }
        self.entries.insert(
            at,
            Entry::Task {
                id: task,
                line: owned,
            },
        );
        Ok(())
    }

    fn move_task(
        &mut self,
        task: TaskId,
        after: Option<TaskId>,
        to_file: &FilePath,
    ) -> Result<(), StateError> {
        if *to_file != self.path {
            return Err(StateError::Unsupported("cross-file Move"));
        }
        if after == Some(task) {
            return Err(StateError::UnknownTask(task));
        }
        let from = self.index_of(task).ok_or(StateError::UnknownTask(task))?;
        let entry = self.entries.remove(from);
        let at = self.position_after(after)?;
        self.entries.insert(at, entry);
        Ok(())
    }

    fn blank_insert(&mut self, after: Option<TaskId>) -> Result<(), StateError> {
        if self.entries.len() + 1 > MAX_LINES_PER_FILE {
            return Err(StateError::TooManyLines(self.entries.len() + 1));
        }
        let at = self.position_after(after)?;
        self.entries.insert(
            at,
            Entry::Blank(OwnedLine::from_bytes(Vec::new(), self.ending)),
        );
        Ok(())
    }

    fn blank_remove(&mut self, after: Option<TaskId>) -> Result<(), StateError> {
        let at = self.position_after(after)?;
        match self.entries.get(at) {
            Some(Entry::Blank(_)) => {
                self.entries.remove(at);
                Ok(())
            }
            _ => Err(StateError::NoBlank(after)),
        }
    }
}

fn entry_of(index: usize, line: &OwnedLine) -> Result<Entry, StateError> {
    let parsed = line.parse().ok_or(StateError::Opaque(index))?;
    match parsed.kind {
        LineKind::Blank => Ok(Entry::Blank(line.clone())),
        LineKind::Task(task) => {
            let id = task
                .id()
                .map(TaskId::new)
                .ok_or(StateError::MissingId(index))?;
            Ok(Entry::Task {
                id,
                line: line.clone(),
            })
        }
    }
}

/// A `TaskId` from a parsed line, when it has one.
pub fn id_of(line: &OwnedLine) -> Option<TaskId> {
    line.parse().and_then(|l| match l.kind {
        LineKind::Task(t) => t.id().map(TaskId::new),
        LineKind::Blank => None,
    })
}

/// Convenience for tests and the reconciler: a `TaskId` from raw ULID bits.
pub fn task_id(bits: u128) -> TaskId {
    TaskId::new(Ulid::from_u128(bits))
}
