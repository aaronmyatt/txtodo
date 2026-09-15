//! Universal-view DTO (ADR 0025, task `desktop-universal-view`), split out of `dto.rs` the same
//! way `dto_notes.rs`/`dto_tokens.rs`/`dto_pairing.rs`/`dto_activity.rs`/`dto_workspace.rs` already
//! are.

use serde::Serialize;

/// One open (`!completed`) task line from some workspace's root `todo.txt`, tagged with enough
/// workspace identity for the frontend to switch to it and show the owning project in the
/// breadcrumb (`commands_universal::universal_tasks`).
#[derive(Debug, Clone, Serialize)]
pub struct UniversalTaskDto {
    /// ULID text.
    pub workspace_id: String,
    /// Canonicalized absolute path; display-only, mirrors `WorkspaceInfoDto::root`.
    pub workspace_root: String,
    /// 1-based over every line, blanks included — same convention as `TaskRefDto::line_number`.
    pub line_number: u32,
    /// `A`-`Z`, uppercase; absent when the line has no priority.
    pub priority: Option<String>,
    /// `@context` tags, `@` included, in line order; may be empty.
    pub contexts: Vec<String>,
    /// The line's description: everything after the structured `x `/dates/priority prefix.
    pub description: String,
}
