//! Intent-level mutations (what a client asks for) → ops (what the log records). Pure over a
//! `DocState`; the actor stamps, applies and persists. A `TaskRef` names a line two ways so a
//! client that read the file before someone else changed it gets `Stale`, never the wrong line.

use crate::reconcile::change_ops;
use crate::state::{DocState, Entry, id_of};
use std::fmt;
use txtodo_core::{Date, Edit, LineKind, OwnedLine};
use txtodo_model::{Field, FieldValue, FilePath, OpKind, TaskId, set_field};

/// Most mutations one Apply accepts.
pub const MAX_MUTATIONS_PER_APPLY: usize = 10_000;

/// A line as the client saw it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskRef {
    /// 1-based over every line, blanks included (CLI convention).
    pub line_number: usize,
    /// The id the client saw there, when it had one.
    pub task_id: Option<TaskId>,
}

/// What a client can ask for. Cross-file `Move` is M5.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mutation {
    /// Append a task line; an `id:` is added when missing.
    Add {
        /// Line text without ending.
        line: String,
    },
    /// Mark done on `today`; a priority becomes `pri:` (core `Edit::complete`).
    Complete {
        /// The line.
        task: TaskRef,
        /// The client's local date (ADR 0011).
        today: Date,
    },
    /// Replace the whole line text; the id must survive.
    Edit {
        /// The line.
        task: TaskRef,
        /// New text without ending.
        new_line: String,
    },
    /// Move to another document (M5).
    Move {
        /// The line.
        task: TaskRef,
        /// Destination.
        to: FilePath,
    },
    /// Remove the line, optionally leaving a blank (todo.sh `del` default).
    Delete {
        /// The line.
        task: TaskRef,
        /// Keep line numbers stable with a blank.
        leave_blank: bool,
    },
}

/// Why a mutation was refused. All are client errors except `TooMany`, which is a limit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MutationError {
    /// No such line number.
    NoLine(usize),
    /// The line is blank.
    Blank(usize),
    /// The id at that line differs from what the client saw.
    Stale {
        /// Line number.
        line_number: usize,
        /// What the client sent.
        expected: TaskId,
        /// What is there now.
        found: TaskId,
    },
    /// The text is not a task line (empty, or a line break).
    NotATask(String),
    /// The edit dropped or changed the `id:` tag.
    IdChanged(TaskId),
    /// Not supported on one device in M3.
    Unsupported(&'static str),
    /// More than `MAX_MUTATIONS_PER_APPLY`.
    TooMany(usize),
}

impl fmt::Display for MutationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MutationError::NoLine(n) => write!(f, "no line {n}"),
            MutationError::Blank(n) => write!(f, "line {n} is blank"),
            MutationError::Stale {
                line_number,
                expected,
                found,
            } => {
                write!(
                    f,
                    "line {line_number} is now task {found}, not {expected}; re-list and retry"
                )
            }
            MutationError::NotATask(t) => write!(f, "{t:?} is not a task line"),
            MutationError::IdChanged(t) => write!(f, "the edit must keep id {t}"),
            MutationError::Unsupported(what) => write!(f, "{what} is not supported yet"),
            MutationError::TooMany(n) => write!(
                f,
                "{n} mutations in one apply, max {MAX_MUTATIONS_PER_APPLY}"
            ),
        }
    }
}

impl std::error::Error for MutationError {}

/// Resolves a `TaskRef` against the state: the entry index and its id.
pub fn resolve(state: &DocState, task: &TaskRef) -> Result<(usize, TaskId), MutationError> {
    let n = task.line_number;
    let i = n.checked_sub(1).ok_or(MutationError::NoLine(n))?;
    let entry = state.entry_at(i).ok_or(MutationError::NoLine(n))?;
    let found = match entry {
        Entry::Task { id, .. } => id,
        Entry::Blank(_) => return Err(MutationError::Blank(n)),
    };
    if let Some(expected) = task.task_id
        && expected != found
    {
        return Err(MutationError::Stale {
            line_number: n,
            expected,
            found,
        });
    }
    debug_assert_eq!(
        state.index_of(found),
        Some(i),
        "ids are unique in a document"
    );
    Ok((i, found))
}

/// Turns one mutation into ops against `state`. `mint` supplies an id for an `Add` without one.
pub fn mutation_ops(
    state: &DocState,
    mutation: &Mutation,
    mint: &mut dyn FnMut() -> TaskId,
) -> Result<Vec<OpKind>, MutationError> {
    match mutation {
        Mutation::Add { line } => add_ops(state, line, mint),
        Mutation::Complete { task, today } => {
            let (_, id) = resolve(state, task)?;
            let old = state
                .line_of(id)
                .ok_or(MutationError::NoLine(task.line_number))?;
            let new = txtodo_core::apply(&old, &Edit::new().complete(*today));
            Ok(change_ops(&old, &new))
        }
        Mutation::Edit { task, new_line } => edit_ops(state, task, new_line),
        Mutation::Move { .. } => Err(MutationError::Unsupported("Move between files")),
        Mutation::Delete { task, leave_blank } => {
            let (i, id) = resolve(state, task)?;
            let after = state.task_before(i);
            let mut ops = vec![
                set_field(id, Field::Deleted, FieldValue::Bool(true))
                    .unwrap_or(OpKind::BlankRemove { after }),
            ];
            if *leave_blank {
                ops.push(OpKind::BlankInsert { after });
            }
            Ok(ops)
        }
    }
}

fn add_ops(
    state: &DocState,
    line: &str,
    mint: &mut dyn FnMut() -> TaskId,
) -> Result<Vec<OpKind>, MutationError> {
    let owned = task_line(line)?;
    let owned = match id_of(&owned) {
        Some(_) => owned,
        None => {
            let tag = mint().ulid().to_string();
            let edit = Edit::new()
                .set_tag("id", &tag)
                .map_err(|_| MutationError::NotATask(line.to_owned()))?;
            txtodo_core::apply(&owned, &edit)
        }
    };
    let task = id_of(&owned).ok_or_else(|| MutationError::NotATask(line.to_owned()))?;
    let after = state.task_before(state.len());
    let text = owned.raw().unwrap_or_default().to_owned();
    debug_assert!(text.contains("id:"), "an added line always carries its id");
    Ok(vec![OpKind::Insert {
        task,
        after,
        line: text,
    }])
}

fn edit_ops(
    state: &DocState,
    task: &TaskRef,
    new_line: &str,
) -> Result<Vec<OpKind>, MutationError> {
    let (_, id) = resolve(state, task)?;
    let new = task_line(new_line)?;
    if id_of(&new) != Some(id) {
        return Err(MutationError::IdChanged(id));
    }
    let old = state
        .line_of(id)
        .ok_or(MutationError::NoLine(task.line_number))?;
    Ok(change_ops(&old, &new))
}

/// Validates client text into a task line (LF ending; the state re-ends it on insert).
fn task_line(text: &str) -> Result<OwnedLine, MutationError> {
    if text.contains('\n') || text.contains('\r') {
        return Err(MutationError::NotATask(text.to_owned()));
    }
    let owned = OwnedLine::from_bytes(text.as_bytes().to_vec(), txtodo_core::LineEnding::default());
    match owned.parse().map(|l| l.kind) {
        Some(LineKind::Task(_)) => Ok(owned),
        _ => Err(MutationError::NotATask(text.to_owned())),
    }
}
