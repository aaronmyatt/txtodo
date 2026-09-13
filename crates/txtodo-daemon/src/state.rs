//! In-memory state of one document: the ordered entries the actor owns, materialised to the exact
//! bytes on disk, mutated only through ops. M4 swaps the backing store for Loro behind this same
//! shape (plan M4), so nothing outside this module touches `entries`.

use crate::textedit::TextEditError;
use std::fmt;
use txtodo_core::{File, LineEnding, LineKind, OwnedLine};
use txtodo_model::{FilePath, IdentityMode, Op, OpKind, TaskId, Ulid};

/// Most lines one document may hold; a 10k-line workspace is the perf target, this is 100× that.
pub const MAX_LINES_PER_FILE: usize = 1_000_000;

/// One line of the document.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Entry {
    /// A task line.
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

/// Task-line counts for one document: blanks are excluded (plan §3.2.5's progress rule).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TaskCounts {
    /// Task lines, blanks excluded.
    pub total: usize,
    /// Of those, lines that start `x ` (completed).
    pub completed: usize,
}

/// Why an op or a file could not be applied. An op that fails here is a daemon bug or a stale
/// client; the message says which task and what was attempted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StateError {
    /// A task line has no resolved id, tag or fingerprint match (line index).
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
    /// The op kind is not handled in this document model (notes, undelete-via-`SetField`).
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
    mode: IdentityMode,
}

impl DocState {
    /// Builds the state from a file and each line's resolved id (`ids[i]`, `None` for a blank).
    pub fn from_file(
        path: FilePath,
        file: &File,
        ids: &[Option<TaskId>],
        mode: IdentityMode,
    ) -> Result<DocState, StateError> {
        if file.lines.len() > MAX_LINES_PER_FILE {
            return Err(StateError::TooManyLines(file.lines.len()));
        }
        debug_assert_eq!(ids.len(), file.lines.len(), "one resolved id per line");
        let mut entries = Vec::with_capacity(file.lines.len());
        for (i, line) in file.lines.iter().enumerate() {
            entries.push(entry_of(i, line, ids.get(i).copied().flatten())?);
        }
        debug_assert_eq!(entries.len(), file.lines.len());
        Ok(DocState {
            path,
            entries,
            bom: file.bom,
            ending: file.ending,
            trailing_newline: file.trailing_newline,
            mode,
        })
    }

    /// The document's path.
    pub fn path(&self) -> &FilePath {
        &self.path
    }

    /// How this document establishes task identity.
    pub fn mode(&self) -> IdentityMode {
        self.mode
    }

    /// The file's line ending.
    pub fn ending(&self) -> LineEnding {
        self.ending
    }

    /// Whether the file starts with a BOM.
    pub fn bom(&self) -> bool {
        self.bom
    }

    /// Number of lines, blanks included.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// True when the document has no lines.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Counts task lines and how many are completed; blanks are excluded (plan §3.2.5, used by the
    /// `ListFiles` RPC's progress field). Re-parses each task's stored bytes rather than the whole
    /// file, since `Entry` keeps completion state in the line text, not as a cached flag.
    pub fn task_counts(&self) -> TaskCounts {
        let mut counts = TaskCounts::default();
        for entry in &self.entries {
            let Entry::Task { line, .. } = entry else {
                continue;
            };
            counts.total += 1;
            if is_completed(line) {
                counts.completed += 1;
            }
        }
        counts
    }

    /// The entry at `i`, by value: the backing store is not a slice after M4 (plan M4), so no
    /// borrowed `&[Entry]` is lent out.
    pub fn entry_at(&self, i: usize) -> Option<Entry> {
        self.entries.get(i).cloned()
    }

    /// The line bytes of a task, if present.
    pub fn line_of(&self, id: TaskId) -> Option<OwnedLine> {
        let i = self.index_of(id)?;
        debug_assert!(i < self.entries.len(), "index_of is in range");
        Some(self.entries[i].line().clone())
    }

    /// The nearest task id strictly before position `i` — the `after` anchor an op at `i` needs.
    /// `i == len()` asks for the last task in the document.
    pub fn task_before(&self, i: usize) -> Option<TaskId> {
        debug_assert!(i <= self.entries.len(), "task_before index {i} in range");
        self.entries[..i.min(self.entries.len())]
            .iter()
            .rev()
            .find_map(Entry::id)
    }

    /// Position of a task, if present.
    pub fn index_of(&self, id: TaskId) -> Option<usize> {
        self.entries.iter().position(|e| e.id() == Some(id))
    }

    /// Every task's id and line, in file order (blanks skipped): item `i` is task-line index `i`.
    pub fn task_lines(&self) -> impl Iterator<Item = (TaskId, &OwnedLine)> {
        self.entries.iter().filter_map(|e| match e {
            Entry::Task { id, line } => Some((*id, line)),
            Entry::Blank(_) => None,
        })
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

    /// Applies one op. On `Err` the state is unchanged. Takes the whole `Op` because the M4 store
    /// arbitrates prefix fields by the op's HLC (ADR 0013); the caller stamps before applying.
    pub fn apply(&mut self, op: &Op) -> Result<(), StateError> {
        let before = self.entries.len();
        match &op.kind {
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

    /// Test seam: applies a bare `OpKind` under a zero stamp. Tests pin bytes, not clocks.
    #[cfg(test)]
    pub(crate) fn apply_kind(&mut self, kind: &OpKind) -> Result<(), StateError> {
        self.apply(&crate::fastid::hydration_op(
            &self.path.clone(),
            kind.clone(),
        ))
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
        if self.mode == IdentityMode::Tagged {
            let parsed_id = owned.parse().and_then(|l| match l.kind {
                LineKind::Task(t) => t.id(),
                LineKind::Blank => None,
            });
            if parsed_id != Some(task.ulid()) {
                return Err(StateError::IdMismatch(task));
            }
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

    /// A same-file reorder moves the entry to its new position; a cross-file move (`to_file !=
    /// self.path`) only removes it here — the destination actor gets its own `Insert` instead
    /// (`crate::move_coordinator`, plan §3.2.8).
    fn move_task(
        &mut self,
        task: TaskId,
        after: Option<TaskId>,
        to_file: &FilePath,
    ) -> Result<(), StateError> {
        if *to_file != self.path {
            let from = self.index_of(task).ok_or(StateError::UnknownTask(task))?;
            self.entries.remove(from);
            return Ok(());
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

/// Whether a task line starts `x ` (todo.txt's completed marker); `false` for anything else,
/// blanks included.
fn is_completed(line: &OwnedLine) -> bool {
    matches!(
        line.parse().map(|l| l.kind),
        Some(LineKind::Task(t)) if t.completed
    )
}

fn entry_of(index: usize, line: &OwnedLine, id: Option<TaskId>) -> Result<Entry, StateError> {
    let parsed = line.parse().ok_or(StateError::Opaque(index))?;
    match parsed.kind {
        LineKind::Blank => Ok(Entry::Blank(line.clone())),
        LineKind::Task(_) => {
            let id = id.ok_or(StateError::MissingId(index))?;
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
