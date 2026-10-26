//! `apply`'s mutations as the frontend sends them (`MutationDto` in
//! `apps/desktop/src-tauri/src/dto.rs`: `{ "kind": "add", ... }`), into the engine's. The same
//! fixes the daemon makes at its wire (`txtodo-daemon`'s `convert.rs::parse_mutation`): a typed
//! `X `/`(a) ` slip is fixed, a line typed done gets today as its done date, and a new line gets
//! today as its creation date (ADR 0011).

use serde::Deserialize;
use txtodo_core::Date;
use txtodo_core::line_fixes::{date_done, fix_case, stamp_created};
use txtodo_engine::mutation::{Mutation, TaskRef};
use txtodo_model::{FilePath, TaskId, Ulid};

use crate::dto::unhex;
use crate::error::WebError;

/// A line: 1-based number and/or ULID text (`""` when the line has none).
#[derive(Debug, Clone, Deserialize)]
pub struct TaskRefDto {
    /// 1-based over every line, blanks included; 0 with an id means "by id alone".
    pub line_number: u32,
    /// ULID text, or `""`.
    pub task_id: String,
}

/// One intent-level mutation, tagged by `kind`.
/// Ref: <https://serde.rs/enum-representations.html#internally-tagged>
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MutationDto {
    /// Appends a full line.
    Add {
        /// The line, no ending.
        line: String,
    },
    /// Marks a line done on `today`.
    Complete {
        /// The line.
        task: TaskRefDto,
        /// `YYYY-MM-DD`, the client's local date.
        today: String,
    },
    /// Replaces a line's text.
    Edit {
        /// The line.
        task: TaskRefDto,
        /// The new text.
        new_line: String,
    },
    /// Moves a line to another list.
    Move {
        /// The line.
        task: TaskRefDto,
        /// The destination list.
        to_path: String,
    },
    /// Deletes a line.
    Delete {
        /// The line.
        task: TaskRefDto,
        /// Leave a blank line where it was.
        leave_blank: bool,
    },
    /// Moves a line right before another.
    MoveBefore {
        /// The line.
        task: TaskRefDto,
        /// The line it lands in front of.
        before: TaskRefDto,
    },
    /// The whole list, compare-and-swap.
    Replace {
        /// Hex blake3 the text was edited from.
        base_hash: String,
        /// The new text.
        contents: String,
    },
}

fn task_ref(t: TaskRefDto) -> Result<TaskRef, WebError> {
    let task_id = match t.task_id.as_str() {
        "" => None,
        text => {
            Some(TaskId::new(Ulid::parse(text).ok_or_else(|| {
                WebError::BadArgs(format!("task id {text:?}"))
            })?))
        }
    };
    if t.line_number == 0 && task_id.is_none() {
        return Err(WebError::BadArgs("line numbers start at 1".to_owned()));
    }
    let line_number =
        usize::try_from(t.line_number).map_err(|_| WebError::BadArgs("line number".to_owned()))?;
    Ok(TaskRef {
        line_number,
        task_id,
    })
}

fn path(text: &str) -> Result<FilePath, WebError> {
    FilePath::new(text).map_err(|e| WebError::BadArgs(format!("path {text:?}: {e}")))
}

impl MutationDto {
    /// The engine's mutation, with the wire's line fixes for `today`.
    pub fn into_mutation(self, today: Date) -> Result<Mutation, WebError> {
        let fix = |line: &str| date_done(&fix_case(line), today);
        Ok(match self {
            MutationDto::Add { line } => Mutation::Add {
                line: stamp_created(&fix(&line), today),
            },
            MutationDto::Complete { task, today } => Mutation::Complete {
                task: task_ref(task)?,
                today: Date::parse(&today)
                    .ok_or_else(|| WebError::BadArgs("today must be YYYY-MM-DD".to_owned()))?,
            },
            MutationDto::Edit { task, new_line } => Mutation::Edit {
                task: task_ref(task)?,
                new_line: fix(&new_line),
                settle: true,
            },
            MutationDto::Move { task, to_path } => Mutation::Move {
                task: task_ref(task)?,
                to: path(&to_path)?,
            },
            MutationDto::Delete { task, leave_blank } => Mutation::Delete {
                task: task_ref(task)?,
                leave_blank,
            },
            MutationDto::MoveBefore { task, before } => Mutation::MoveBefore {
                task: task_ref(task)?,
                before: task_ref(before)?,
            },
            MutationDto::Replace {
                base_hash,
                contents,
            } => Mutation::Replace {
                // A malformed hash matches no document: refused as stale, never guessed at.
                base: <[u8; 32]>::try_from(unhex(&base_hash).as_slice()).unwrap_or([0; 32]),
                contents: contents.into_bytes(),
            },
        })
    }
}
