//! In-memory state of one document: the ordered entries the actor owns, materialised to the exact
//! bytes on disk, mutated only through ops (plan M4 swaps the backing store, same shape).

use std::collections::HashMap;

use txtodo_core::{File, LineEnding, LineKind, OwnedLine};
use txtodo_model::{Field, FilePath, Hlc, IdentityMode, Op, OpKind, TaskId, Ulid};

// Where concurrent placements go (task insert-order): each entry's placing stamp and RGA's skip
// rule. A child module so it can keep `entries` and `stamps` in step without widening them.
#[path = "state_order.rs"]
mod order;
pub(crate) use order::scratch_op;
// Deleted and moved-away placements kept as hidden entries (ADR 0033), and the visible view.
#[path = "state_ghosts.rs"]
mod ghosts;
pub use ghosts::MAX_GHOSTS_PER_FILE;
// A placement that lands late takes the entries that should follow it (ADR 0033).
#[path = "state_rehome.rs"]
mod rehome;
// A `BlankRemove` is kept as an eraser; the blank it hides is settled after every op.
#[path = "state_erase.rs"]
mod erase;

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

/// Task-line counts for one document, blanks excluded (plan §3.2.5).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TaskCounts {
    /// Task lines, blanks excluded.
    pub total: usize,
    /// Of those, lines that start `x ` (completed).
    pub completed: usize,
}

pub use crate::state_error::StateError;

/// `apply`'s span field: the op kind's name, never its payload; blank/notes group as `"other"`.
fn op_kind_name(kind: &OpKind) -> &'static str {
    match kind {
        OpKind::Insert { .. } => "insert",
        OpKind::SetField { .. } => "set_field",
        OpKind::EditText { .. } => "edit_text",
        OpKind::Move { .. } => "move",
        _ => "other",
    }
}
fn log_state_applied(entries: usize) {
    tracing::trace!(entries, "state_applied");
}

/// The document.
#[derive(Clone, Debug)]
pub struct DocState {
    path: FilePath,
    /// Every placement, shown or not: `hidden[s]` marks a ghost (ADR 0033, `state_ghosts.rs`).
    entries: Vec<Entry>,
    hidden: Vec<bool>,
    /// The slots shown, in order: line `i` is `entries[visible[i]]`.
    visible: Vec<usize>,
    /// `stamps[i]`: the HLC of the op that placed `entries[i]`, zero for a line read from disk.
    /// Merge metadata, not content: equality ignores it (task insert-order, `state_order.rs`).
    stamps: Vec<Hlc>,
    /// `parents[i]`: the anchor placement the op that placed `entries[i]` followed
    /// (`state_rehome.rs`). Merge metadata, like `stamps`.
    parents: Vec<order::Parent>,
    /// `erasers[i]`: `entries[i]` is a `BlankRemove`, never shown (`state_erase.rs`).
    erasers: Vec<bool>,
    /// The stamp of the `SetField` each task's field last took, so an older one arriving late
    /// loses on every device (task partition-converge). Merge metadata, like `stamps`.
    field_stamps: HashMap<(TaskId, Field), Hlc>,
    /// Each task's description edits since its history began, so a late one is slotted in by
    /// stamp (ADR 0034, `text_history.rs`). Merge metadata, like `stamps`.
    text_history: HashMap<TaskId, crate::text_history::TextHistory>,
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
        let stamps = vec![order::unplaced(); entries.len()];
        Ok(DocState {
            path,
            hidden: vec![false; entries.len()],
            parents: vec![None; entries.len()],
            erasers: vec![false; entries.len()],
            visible: (0..entries.len()).collect(),
            entries,
            stamps,
            field_stamps: HashMap::new(),
            text_history: HashMap::new(),
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

    /// Counts task lines and completions, blanks excluded (plan §3.2.5's `ListFiles` progress).
    pub fn task_counts(&self) -> TaskCounts {
        let mut counts = TaskCounts::default();
        for (_, line) in self.task_lines() {
            counts.total += 1;
            counts.completed += usize::from(is_completed(line));
        }
        counts
    }

    /// Applies one op (unchanged on `Err`); a thin span wrapper (see `apply_inner`'s own doc).
    #[tracing::instrument(skip_all, fields(kind = op_kind_name(&op.kind)))]
    pub fn apply(&mut self, op: &Op) -> Result<(), StateError> {
        self.apply_inner(op)
    }

    fn apply_inner(&mut self, op: &Op) -> Result<(), StateError> {
        let before = self.entries.len();
        match &op.kind {
            OpKind::Insert { task, after, line } => self.insert(*task, *after, line, op.hlc)?,
            OpKind::SetField { task, field, value } => {
                crate::fields::set_field(self, *task, (*field, *value), op.hlc)?
            }
            OpKind::EditText { task, edits } => {
                crate::fields::edit_text(self, *task, edits, op.hlc)?
            }
            OpKind::Move {
                task,
                after,
                to_file,
            } => self.move_task(*task, *after, to_file, op.hlc)?,
            OpKind::BlankInsert { after } => self.blank_insert(*after, op.hlc)?,
            OpKind::BlankRemove { after } => self.blank_remove(*after, op.hlc)?,
            OpKind::NotesEdit { .. } => return Err(StateError::Unsupported("NotesEdit")),
        }
        debug_assert!(self.visible.len() <= MAX_LINES_PER_FILE);
        debug_assert!(self.entries.len() >= before, "an op hides, never removes");
        if reshapes(&op.kind) {
            self.settle_erasers();
        }
        self.prune_ghosts();
        log_state_applied(self.visible.len());
        Ok(())
    }

    /// Test seam: applies a bare `OpKind` as the newest op would land. Tests pin bytes, not clocks.
    #[cfg(test)]
    pub(crate) fn apply_kind(&mut self, kind: &OpKind) -> Result<(), StateError> {
        self.apply(&scratch_op(&self.path.clone(), kind.clone()))
    }

    fn insert(
        &mut self,
        task: TaskId,
        after: Option<TaskId>,
        line: &str,
        hlc: Hlc,
    ) -> Result<(), StateError> {
        if self.visible.len() + 1 > MAX_LINES_PER_FILE {
            return Err(StateError::TooManyLines(self.visible.len() + 1));
        }
        let spot = self.slot_after(after, hlc)?;
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
        let entry = Entry::Task {
            id: task,
            line: owned,
        };
        self.insert_entry(spot, entry, hlc);
        Ok(())
    }

    /// Same-file: the task's shown placement becomes a ghost where it was and a new one goes
    /// after `after` (ADR 0033). The anchor is checked before anything moves (`apply` is unchanged
    /// on `Err`, task sync-poison-op). A move older than the task's shown placement, or of a task
    /// already deleted, adds its spot as a ghost only, so the newest of two concurrent moves wins
    /// and every device holds the same placements. Cross-file: hides only — the destination has
    /// its `Insert`.
    fn move_task(
        &mut self,
        task: TaskId,
        after: Option<TaskId>,
        to_file: &FilePath,
        hlc: Hlc,
    ) -> Result<(), StateError> {
        if *to_file != self.path {
            return match self.live_slot(task) {
                Some(from) => {
                    self.hide_entry(from);
                    self.forget_fields(task);
                    Ok(())
                }
                None if self.has_placement(task) => Ok(()),
                None => Err(StateError::UnknownTask(task)),
            };
        }
        if after == Some(task) {
            return Err(StateError::UnknownTask(task));
        }
        self.anchor_slot(after, hlc)?;
        let Some(from) = self.live_slot(task) else {
            return self.place_ghost(task, after, hlc);
        };
        if self.is_stale_move(from, hlc) {
            order::log_stale_move(task);
            return self.place_ghost(task, after, hlc);
        }
        let entry = self.entries[from].clone();
        self.hide_entry(from);
        let spot = self.slot_after(after, hlc)?;
        self.insert_entry(spot, entry, hlc);
        self.rehome_onto(task, hlc);
        Ok(())
    }

    fn blank_insert(&mut self, after: Option<TaskId>, hlc: Hlc) -> Result<(), StateError> {
        if self.visible.len() + 1 > MAX_LINES_PER_FILE {
            return Err(StateError::TooManyLines(self.visible.len() + 1));
        }
        let spot = self.slot_after(after, hlc)?;
        let entry = Entry::Blank(OwnedLine::from_bytes(Vec::new(), self.ending));
        self.insert_entry(spot, entry, hlc);
        Ok(())
    }

    /// Places an eraser after the anchor (`state_erase.rs`). The blank it hides is settled with
    /// every other eraser's after each op, so a blank or an anchor placement that lands later
    /// still ends with the blank every device hides. One that finds no blank is kept all the same.
    fn blank_remove(&mut self, after: Option<TaskId>, hlc: Hlc) -> Result<(), StateError> {
        let spot = self.slot_after(after, hlc)?;
        let at = spot.at;
        let entry = Entry::Blank(OwnedLine::from_bytes(Vec::new(), self.ending));
        self.insert_entry(spot, entry, hlc);
        self.erasers[at] = true;
        self.hide_entry(at);
        Ok(())
    }
}

/// Whether an op can change which blank an eraser claims: it places, hides or removes a line.
fn reshapes(kind: &OpKind) -> bool {
    match kind {
        OpKind::SetField { field, .. } => *field == Field::Deleted,
        OpKind::EditText { .. } | OpKind::NotesEdit { .. } => false,
        _ => true,
    }
}

/// Whether a task line starts `x ` (completed); `false` for anything else, blanks included.
pub(crate) fn is_completed(line: &OwnedLine) -> bool {
    matches!(line.parse().map(|l| l.kind), Some(LineKind::Task(t)) if t.completed)
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
