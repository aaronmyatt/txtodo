//! Tool argument structs and their small helper types, split out of `backend.rs` for the file
//! budget (same pattern as `txtodo-daemon`'s `workspace_catalog.rs`/`workspace_catalog_open.rs`
//! split). Re-exported from `backend.rs` (`pub use backend_args::*`) so every existing
//! `crate::backend::<Type>` import keeps working unchanged.

use serde::{Deserialize, Serialize};

use crate::backend::{Hlc, RefPath, TaskId};
use crate::error::McpError;

/// Which registered workspace a call targets: a `todo_list_workspaces` `id` (a `WorkspaceId` ULID),
/// or a filesystem path — mirrors the daemon's own `WorkspaceSelector` oneof (`txtodo.proto`), but
/// as one string rather than two fields, since a caller only ever has one of the two in hand at a
/// time (see this crate's As-built notes for why). Omitted resolves to "the sole open workspace":
/// ambiguous, refused by the daemon with a clear error, when 0 or 2+ are open
/// (`workspace_catalog.rs::resolve`'s own convention — this crate adds no guessing on top of it).
pub type WorkspaceArg = Option<String>;

/// `todo_list` args.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ListArgs {
    /// Whitespace-separated terms with `txtodo list`'s matching: every term must match, a term is
    /// a case-insensitive substring of the line (`+project` and `@context` included), and a
    /// leading `-` excludes lines containing the rest — see [`crate::parse::matches_query`]. The
    /// design §8 query language (`txtodo-query`) is still a stub.
    pub query: Option<String>,
    /// Workspace-relative ref path; defaults to `todo.txt`.
    pub file: Option<RefPath>,
    /// Caps the number of rows returned; 0/absent = daemon default.
    pub limit: Option<u32>,
    /// Which registered workspace this targets; see [`WorkspaceArg`].
    pub workspace: WorkspaceArg,
}

/// `todo_search` args.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SearchArgs {
    /// The text to search for.
    pub text: String,
    /// Workspace-relative ref path; defaults to `todo.txt`.
    pub file: Option<RefPath>,
    /// Which registered workspace this targets; see [`WorkspaceArg`].
    pub workspace: WorkspaceArg,
}

/// `todo_get` args: `id` and `line` may both be given (the daemon rejects a mismatch), but at
/// least one is required.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct GetTarget {
    /// The task's id, as `TaskRow.id` gave it.
    pub id: Option<TaskId>,
    /// A 1-based line number, blanks included.
    pub line: Option<u32>,
    /// Workspace-relative ref path; defaults to `todo.txt`. Only meaningful with `line`, since an
    /// `id` is resolved against the whole workspace.
    pub file: Option<RefPath>,
    /// Which registered workspace this targets; see [`WorkspaceArg`].
    pub workspace: WorkspaceArg,
}

/// `todo_edit` args: a field-level patch, never a whole-line rewrite (design §6.3).
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct FieldPatch {
    /// A single letter A-Z to set, or `""` to clear the leading `(X)`/`pri:` priority.
    pub priority: Option<String>,
    /// A date to set, or `""` to remove the `due:` tag.
    pub due: Option<String>,
    /// Text appended to the end of the line (todo.sh `append` semantics: no leading space before
    /// a leading sentence delimiter).
    pub append: Option<String>,
    /// Replaces the line's body, keeping the existing priority/date prefix (todo.sh `replace`).
    pub replace: Option<String>,
}

/// `todo_edit` tool args (the trait method above takes `id`/`patch` separately; this bundles them
/// for the single JSON-object tool call).
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct EditArgs {
    /// The task to edit.
    pub id: TaskId,
    /// The field-level patch.
    pub patch: FieldPatch,
    /// Which registered workspace this targets; see [`WorkspaceArg`].
    pub workspace: WorkspaceArg,
}

/// `todo_move` args.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct MoveArgs {
    /// The task to move.
    pub id: TaskId,
    /// Reorder before this task id. Exactly one of `before`/`after` is required.
    pub before: Option<TaskId>,
    /// Reorder after this task id. Exactly one of `before`/`after` is required.
    pub after: Option<TaskId>,
    /// Which registered workspace this targets; see [`WorkspaceArg`].
    pub workspace: WorkspaceArg,
}

/// Which side of an anchor task [`crate::backend::McpBackend::move_task`] reorders next to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MoveAnchor {
    /// Immediately before this task.
    Before(TaskId),
    /// Immediately after this task.
    After(TaskId),
}

/// Exactly one of `before`/`after` is required (`todo_move`'s args and `TodoOp::TodoMove` share
/// this check).
pub fn move_anchor(before: Option<TaskId>, after: Option<TaskId>) -> Result<MoveAnchor, McpError> {
    match (before, after) {
        (Some(b), None) => Ok(MoveAnchor::Before(b)),
        (None, Some(a)) => Ok(MoveAnchor::After(a)),
        _ => Err(McpError::invalid_params(
            "todo_move needs exactly one of before/after",
        )),
    }
}

/// One operation inside a `todo_batch` call. Tagged exactly like the tool names (design §6.6's
/// example session): `{"todo_add": {...}}`, `{"todo_edit": {...}}`, etc. No per-op `workspace`:
/// one selector applies to the whole `todo_batch` call (`BatchArgs.workspace`) — see this crate's
/// As-built notes for why stacking a second per-op axis was left out.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TodoOp {
    /// `todo_add`.
    TodoAdd {
        /// The raw line text, minus dates (the daemon stamps `created:`/`id:`).
        text: String,
        /// Workspace-relative ref path; defaults to `todo.txt`.
        file: Option<RefPath>,
    },
    /// `todo_complete`.
    TodoComplete {
        /// The task to complete.
        id: TaskId,
    },
    /// `todo_uncomplete`.
    TodoUncomplete {
        /// The task to reopen.
        id: TaskId,
    },
    /// `todo_edit`.
    TodoEdit {
        /// The task to edit.
        id: TaskId,
        /// The field-level patch.
        patch: FieldPatch,
    },
    /// `todo_move`.
    TodoMove {
        /// The task to move.
        id: TaskId,
        /// Reorder before this task id.
        before: Option<TaskId>,
        /// Reorder after this task id.
        after: Option<TaskId>,
    },
    /// `todo_delete`.
    TodoDelete {
        /// The task to delete.
        id: TaskId,
        /// Must be `true` (design §6.3 invariant); asserted, never trusted.
        confirm: bool,
    },
}

/// `todo_add` args.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct AddArgs {
    /// The raw line text, minus dates (the daemon stamps `created:`/`id:`).
    pub text: String,
    /// Workspace-relative ref path; defaults to `todo.txt`.
    pub file: Option<RefPath>,
    /// Which registered workspace this targets; see [`WorkspaceArg`].
    pub workspace: WorkspaceArg,
}

/// `todo_complete` / `todo_uncomplete` args.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct IdArgs {
    /// The task.
    pub id: TaskId,
    /// Which registered workspace this targets; see [`WorkspaceArg`].
    pub workspace: WorkspaceArg,
}

/// `todo_delete` args.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct DeleteArgs {
    /// The task to delete.
    pub id: TaskId,
    /// Must be `true` (design §6.3 invariant); asserted, never trusted.
    pub confirm: bool,
    /// Which registered workspace this targets; see [`WorkspaceArg`].
    pub workspace: WorkspaceArg,
}

/// `todo_archive` args.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ArchiveArgs {
    /// Workspace-relative ref path; defaults to `todo.txt`.
    pub file: Option<RefPath>,
    /// Which registered workspace this targets; see [`WorkspaceArg`].
    pub workspace: WorkspaceArg,
}

/// `todo_batch` args.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct BatchArgs {
    /// The operations to apply, in order.
    pub ops: Vec<TodoOp>,
    /// `true` declares a preview only; see [`crate::backend::McpBackend::batch`]'s doc for what
    /// that does today.
    pub dry_run: bool,
    /// Which registered workspace every op in `ops` targets; see [`WorkspaceArg`].
    pub workspace: WorkspaceArg,
}

/// `todo_history` args.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct HistoryArgs {
    /// Only ops at or after this HLC.
    pub since: Option<Hlc>,
    /// Only ops touching this task.
    pub id: Option<TaskId>,
    /// Workspace-relative ref path; every document when absent.
    pub file: Option<RefPath>,
    /// Which registered workspace this targets; see [`WorkspaceArg`].
    pub workspace: WorkspaceArg,
}

/// `todo_raw` args: `lines` for read mode, `line` + `text` for write mode.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct RawArgs {
    /// Workspace-relative ref path.
    pub file: RefPath,
    /// Read mode: 1-based line numbers to return.
    pub lines: Option<Vec<u32>>,
    /// Write mode: the 1-based line number to replace.
    pub line: Option<u32>,
    /// Write mode: the new line text.
    pub text: Option<String>,
    /// Which registered workspace this targets; see [`WorkspaceArg`].
    pub workspace: WorkspaceArg,
}

/// `todo_notes_get` args.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct NotesGetArgs {
    /// The task whose `ref:` directory owns the notes.
    pub id: TaskId,
    /// Which registered workspace this targets; see [`WorkspaceArg`].
    pub workspace: WorkspaceArg,
}

/// `todo_notes_set` args.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct NotesSetArgs {
    /// The task whose `ref:` directory owns the notes.
    pub id: TaskId,
    /// The whole new document text.
    pub text: String,
    /// Which registered workspace this targets; see [`WorkspaceArg`].
    pub workspace: WorkspaceArg,
}

/// `todo_lint` args.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct LintArgs {
    /// Workspace-relative ref path; defaults to `todo.txt`.
    pub file: Option<RefPath>,
    /// Which registered workspace this targets; see [`WorkspaceArg`].
    pub workspace: WorkspaceArg,
}

/// `todo_conflicts_list` args.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ConflictsListArgs {
    /// Workspace-relative ref path; defaults to `todo.txt`.
    pub file: Option<RefPath>,
    /// Which registered workspace this targets; see [`WorkspaceArg`].
    pub workspace: WorkspaceArg,
}

/// Which side of a merge conflict wins (`todo_conflicts_resolve`), mirroring the daemon's
/// `Resolution`: this device's text at flag time, the peer's, or what is already in the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ConflictSide {
    /// This device's description when the two sides diverged.
    Mine,
    /// The peer's description when the two sides diverged.
    Theirs,
    /// Keep the file as it is and only clear the flag; nothing is written.
    Merged,
}

/// `todo_conflicts_resolve` args.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ConflictsResolveArgs {
    /// The conflicted task.
    pub id: TaskId,
    /// Which side to keep.
    pub side: ConflictSide,
    /// Workspace-relative ref path; defaults to the document the task is found in.
    pub file: Option<RefPath>,
    /// Which registered workspace this targets; see [`WorkspaceArg`].
    pub workspace: WorkspaceArg,
}
