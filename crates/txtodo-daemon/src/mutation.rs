//! Intent-level mutations (what a client asks for) → ops (what the log records). Pure over a
//! `DocState`; the actor stamps, applies and persists. A `TaskRef` names a line two ways so a
//! client that read the file before someone else changed it gets `Stale`, never the wrong line.

use crate::expected::Hash;
pub use crate::mutation_moves::{PeekedLine, peek_line};
use crate::mutation_moves::{move_before_ops, move_ops, move_to_end_ops};
use crate::reconcile::change_ops;
use crate::state::{DocState, Entry, id_of};
use std::fmt;
use txtodo_core::{Date, Edit, LineKind, OwnedLine};
use txtodo_model::{Field, FieldValue, FilePath, IdentityMode, OpKind, TaskId, set_field};

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

/// What a client can ask for. `Move` always names another document (plan §3.2.8); the daemon
/// appends the line at the destination's end. It must be the only mutation in its `Apply` batch —
/// `crate::move_coordinator` is what actually moves it, coordinating the two documents' actors and
/// the task's `ref:` directory, so `mutation_ops` below only ever sees the *source* half of it.
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
    /// Move to another document (plan §3.2.8, root todo.txt task 16). Always cross-file: a
    /// same-file reorder is not part of this client API.
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
    /// Moves the line within its own file to sit after the last other task (archiving: a
    /// completed task stays in its file, pushed to the bottom, instead of moving to a second
    /// one). A no-op when the task is already last.
    MoveToEnd {
        /// The line.
        task: TaskRef,
    },
    /// Moves the line within its own file to sit immediately before another task (task
    /// `mcp-move-reorder`): the same-file relocation `MoveToEnd` is the "after the last task"
    /// case of. A blank line between the two stays where it was. Refused if `before` is the task
    /// itself; a no-op when the task already sits right before `before`.
    MoveBefore {
        /// The line to move.
        task: TaskRef,
        /// The task it lands in front of.
        before: TaskRef,
    },
    /// Replaces the whole document, but only if it still hashes to `base` (`replace.rs`): the
    /// caller's compare-and-swap for a diff no other mutation can express. Must be alone in its
    /// batch, and reconciled like an external edit, so untouched lines keep their identity.
    Replace {
        /// The hash the caller edited from (`Contents::hash`).
        base: Hash,
        /// The whole new document.
        contents: Vec<u8>,
    },
    /// A precondition, not a change (`replace.rs`): the batch is refused unless the document still
    /// hashes to `base`. For batches that address lines by number alone, with no `id:` to check
    /// (sidecar mode). Must be the batch's first mutation, and its only guard.
    RequireBase {
        /// The hash the batch was built from (`Contents::hash`).
        base: Hash,
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
    /// A `Replace` named a base hash that is no longer the document's, or the file holds an edit
    /// the daemon has not reconciled yet.
    StaleBase,
    /// The text is not a task line (empty, or a line break).
    NotATask(String),
    /// The edit dropped or changed the `id:` tag.
    IdChanged(TaskId),
    /// A batch mixed a cross-file `Move` with other mutations, or named an op this crate does
    /// not support (`NotesEdit`, undelete-via-`SetField`; see this crate's CLAUDE.md).
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
            MutationError::StaleBase => write!(
                f,
                "the document changed since it was read; nothing was written, re-read and retry"
            ),
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

/// Resolves a `TaskRef` against the state: the entry index and its id. `task.task_id` (parsed by
/// the caller from the last text it read) is a staleness guard against a concurrent edit; only
/// meaningful in tagged mode, where that text is really the daemon's own id — a client's parse of
/// sidecar-mode text carries no such thing, id: substrings there are just ordinary words.
pub fn resolve(state: &DocState, task: &TaskRef) -> Result<(usize, TaskId), MutationError> {
    let n = task.line_number;
    let i = n.checked_sub(1).ok_or(MutationError::NoLine(n))?;
    let entry = state.entry_at(i).ok_or(MutationError::NoLine(n))?;
    let found = match entry {
        Entry::Task { id, .. } => id,
        Entry::Blank(_) => return Err(MutationError::Blank(n)),
    };
    if state.mode() == IdentityMode::Tagged
        && let Some(expected) = task.task_id
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

/// `mutation_ops`'s span field — the variant's name only, never its line text.
fn mutation_kind(m: &Mutation) -> &'static str {
    match m {
        Mutation::Add { .. } => "add",
        Mutation::Complete { .. } => "complete",
        Mutation::Edit { .. } => "edit",
        Mutation::Move { .. } => "move",
        Mutation::Delete { .. } => "delete",
        Mutation::MoveToEnd { .. } => "move_to_end",
        Mutation::MoveBefore { .. } => "move_before",
        Mutation::Replace { .. } => "replace",
        Mutation::RequireBase { .. } => "require_base",
    }
}

/// Split out so the event macro doesn't count against `mutation_ops`'s own `#[instrument]` budget.
fn log_mutation_ops(ops: usize) {
    tracing::debug!(ops, "mutation_ops");
}

/// Turns one mutation into ops against `state`. `mint` supplies an id for an `Add` without one. A
/// thin span wrapper around `mutation_ops_inner` (`#[instrument]` on the real body overflows).
#[tracing::instrument(skip_all, fields(kind = mutation_kind(mutation)))]
pub fn mutation_ops(
    state: &DocState,
    mutation: &Mutation,
    mint: &mut dyn FnMut() -> TaskId,
) -> Result<Vec<OpKind>, MutationError> {
    let ops = mutation_ops_inner(state, mutation, mint)?;
    log_mutation_ops(ops.len());
    Ok(ops)
}

fn mutation_ops_inner(
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
            Ok(change_ops(&old, &new, id))
        }
        Mutation::Edit { task, new_line } => edit_ops(state, task, new_line),
        Mutation::Move { task, to } => move_ops(state, task, to),
        Mutation::MoveToEnd { task } => move_to_end_ops(state, task),
        Mutation::MoveBefore { task, before } => move_before_ops(state, task, before),
        // The actor takes a lone `Replace` before any op is derived (`replace.rs`); one reaching
        // here rode in a batch with other mutations.
        Mutation::Replace { .. } => Err(MutationError::Unsupported(
            "Replace must be its own Apply batch",
        )),
        // Checked by the actor before any op is derived (`replace.rs`'s `guard_batch`).
        Mutation::RequireBase { .. } => Ok(Vec::new()),
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
    let (task, owned) = if state.mode() == IdentityMode::Sidecar {
        (mint(), owned)
    } else {
        match id_of(&owned) {
            Some(id) => (id, owned),
            None => {
                let id = mint();
                let tag = id.ulid().to_string();
                let edit = Edit::new()
                    .set_tag("id", &tag)
                    .map_err(|_| MutationError::NotATask(line.to_owned()))?;
                (id, txtodo_core::apply(&owned, &edit))
            }
        }
    };
    let after = state.task_before(state.len());
    let text = owned.raw().unwrap_or_default().to_owned();
    debug_assert!(
        state.mode() != IdentityMode::Tagged || text.contains("id:"),
        "an added line always carries its id in tagged mode"
    );
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
    if state.mode() == IdentityMode::Tagged && id_of(&new) != Some(id) {
        return Err(MutationError::IdChanged(id));
    }
    let old = state
        .line_of(id)
        .ok_or(MutationError::NoLine(task.line_number))?;
    Ok(change_ops(&old, &new, id))
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
