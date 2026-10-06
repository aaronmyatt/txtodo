//! Universal-view DTO (ADR 0025, task `desktop-universal-view`), split out of `dto.rs` the same
//! way `dto_notes.rs`/`dto_tokens.rs`/`dto_pairing.rs`/`dto_activity.rs`/`dto_workspace.rs` already
//! are.

use serde::Serialize;

/// One task line from some workspace's root `todo.txt` (open ones, plus done ones when asked),
/// tagged with enough
/// workspace identity for the frontend to switch to it and show the owning project in the
/// breadcrumb (`commands_universal::universal_tasks`).
#[derive(Debug, Clone, Serialize)]
pub struct UniversalTaskDto {
    /// ULID text.
    pub workspace_id: String,
    /// Canonicalized absolute path; display-only, mirrors `WorkspaceInfoDto::root`.
    pub workspace_root: String,
    /// The workspace's root list, relative to its root (its layout's `todo_file`): the file
    /// `line_number` counts in, so a click-through opens the right document.
    pub root_list: String,
    /// 1-based over every line, blanks included — same convention as `TaskRefDto::line_number`.
    pub line_number: u32,
    /// `A`-`Z`, uppercase; absent when the line has no priority.
    pub priority: Option<String>,
    /// `@context` tags, `@` included, in line order; may be empty.
    pub contexts: Vec<String>,
    /// The line's description: everything after the structured `x `/dates/priority prefix.
    pub description: String,
    /// The line starts with `x `.
    pub done: bool,
    /// `YYYY-MM-DD` after the `x`, when there is one.
    pub completion_date: Option<String>,
    /// The `due:` tag's value as written (the frontend reads it as a local date, ADR 0011).
    pub due: Option<String>,
    /// `+project` tags, `+` included, in line order; may be empty.
    pub projects: Vec<String>,
    /// The line's task id (ULID text) from `GetFile`'s `task_ids`, for a `TaskRef` on that
    /// workspace; `""` from a daemon older than the field.
    pub task_id: String,
    /// The `ref:` sub-list's progress, when the line has a `ref:` whose list exists.
    pub ref_progress: Option<RefProgressDto>,
}

/// A `ref:` sub-list's `done` of `total` task lines (the daemon's `ListFiles` counts).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct RefProgressDto {
    /// Done task lines in the sub-list.
    pub done: u32,
    /// Task lines in the sub-list, blanks excluded.
    pub total: u32,
}
