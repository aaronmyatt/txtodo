//! The [`McpBackend`] trait (mcp-server-tools notes.md "Design"): schemas and transports
//! (`schema.rs`/`transport.rs`) never touch the daemon directly, only through this seam. The
//! concrete implementation ([`crate::grpc_backend::GrpcMcpBackend`]) calls the existing
//! `Apply`/`GetFile`/`ListFiles`/`History`/`GetNotes`/`EditNotes` daemon RPCs.
//!
//! Two methods here — `list_files`/`get_file` — are not in the notes.md sketch of this trait: they
//! back the `todotxt://todo.txt` resource and the file list, both needing a whole file's bytes
//! rather than a single task. Both are thin wrappers over the daemon's existing `ListFiles`/
//! `GetFile` RPCs (no new daemon surface), so they stay in scope for this task.

use serde::{Deserialize, Serialize};

use crate::error::McpError;

/// A workspace-relative ref path, e.g. `todo.txt` or `q4-roadmap/todo.txt` (design §6.3 `file`).
/// Passed straight through to the daemon's path-resolving RPCs; the daemon's walker
/// (`crates/txtodo-daemon/src/walker.rs`) normalizes it, never the client (notes.md "`file` args
/// take a ref path").
pub type RefPath = String;

/// A task's stable id: the `id:<ULID>` tag's value.
pub type TaskId = String;

/// One parsed todo.txt line (design §6.3): `raw` plus the fields the tool table promises.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskRow {
    /// The `id:` tag's value, when present.
    pub id: Option<TaskId>,
    /// 1-based line number, blanks included.
    pub line: u32,
    /// The untouched line text.
    pub raw: String,
    /// Starts with `x `.
    pub done: bool,
    /// The leading `(X)` priority, when present (moves to the `pri:` tag once done).
    pub priority: Option<char>,
    /// The creation date, when present.
    pub created: Option<String>,
    /// The completion date, when `done`.
    pub completed: Option<String>,
    /// The `due:` tag's value, when present.
    pub due: Option<String>,
    /// `+project` tokens, in line order.
    pub projects: Vec<String>,
    /// `@context` tokens, in line order.
    pub contexts: Vec<String>,
    /// Every other `key:value` tag, in line order (`id`/`due`/`pri` excluded — they have their own
    /// field above).
    pub kv: Vec<(String, String)>,
}

/// `todo_list` args.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ListArgs {
    /// Query language, design §8. `txtodo-query` (the real implementation) is still a stub — see
    /// [`crate::parse::matches_minimal_query`] for what is actually evaluated today.
    pub query: Option<String>,
    /// Workspace-relative ref path; defaults to `todo.txt`.
    pub file: Option<RefPath>,
    /// Caps the number of rows returned; 0/absent = daemon default.
    pub limit: Option<u32>,
}

/// `todo_search` args.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SearchArgs {
    /// The text to search for.
    pub text: String,
    /// Workspace-relative ref path; defaults to `todo.txt`.
    pub file: Option<RefPath>,
}

/// `todo_get` args: `id` and `line` may both be given (the daemon rejects a mismatch), but at
/// least one is required.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct GetTarget {
    /// The task's `id:` tag.
    pub id: Option<TaskId>,
    /// A 1-based line number, blanks included.
    pub line: Option<u32>,
    /// Workspace-relative ref path; defaults to `todo.txt`. Only meaningful with `line`, since an
    /// `id` is resolved against the whole workspace.
    pub file: Option<RefPath>,
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
}

/// Which side of an anchor task [`McpBackend::move_task`] reorders next to.
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
/// example session): `{"todo_add": {...}}`, `{"todo_edit": {...}}`, etc.
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

/// One HLC timestamp (design §4.4): wall time plus the tie-breaking counter.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema,
)]
pub struct Hlc {
    /// Wall time in milliseconds.
    pub wall_ms: u64,
    /// The counter that breaks ties within the same millisecond.
    pub counter: u32,
}

/// `todo_add` args.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct AddArgs {
    /// The raw line text, minus dates (the daemon stamps `created:`/`id:`).
    pub text: String,
    /// Workspace-relative ref path; defaults to `todo.txt`.
    pub file: Option<RefPath>,
}

/// `todo_complete` / `todo_uncomplete` args.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct IdArgs {
    /// The task.
    pub id: TaskId,
}

/// `todo_delete` args.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct DeleteArgs {
    /// The task to delete.
    pub id: TaskId,
    /// Must be `true` (design §6.3 invariant); asserted, never trusted.
    pub confirm: bool,
}

/// `todo_archive` args.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ArchiveArgs {
    /// Workspace-relative ref path; defaults to `todo.txt`.
    pub file: Option<RefPath>,
}

/// `todo_batch` args.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct BatchArgs {
    /// The operations to apply, in order.
    pub ops: Vec<TodoOp>,
    /// `true` declares a preview only; see [`McpBackend::batch`]'s doc for what that does today.
    pub dry_run: bool,
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
}

/// `todo_notes_get` args.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct NotesGetArgs {
    /// The task whose `ref:` directory owns the notes.
    pub id: TaskId,
}

/// `todo_notes_set` args.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct NotesSetArgs {
    /// The task whose `ref:` directory owns the notes.
    pub id: TaskId,
    /// The whole new document text.
    pub text: String,
}

/// `todo_archive`/`todo_batch` outcome.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ApplyOutcome {
    /// Ops appended.
    pub applied: u32,
    /// The projection hash after the write, hex-encoded; absent when nothing was applied (a
    /// `dry_run` `todo_batch` call, or an archive with nothing completed).
    pub hash: Option<String>,
    /// The HLC of the write, when one happened.
    pub hlc: Option<Hlc>,
    /// Set only by a real diff renderer ([mcp-batch-dry-run](../../../tasks/mcp-batch-dry-run));
    /// always `None` here — see [`McpBackend::batch`]'s doc.
    pub diff: Option<String>,
}

/// One line from the op log (design §6.3 `todo_history`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpSummary {
    /// Monotonic sequence number within the document.
    pub seq: i64,
    /// ULID text.
    pub op_id: String,
    /// When the op was stamped.
    pub hlc: Hlc,
    /// The device that stamped it (ULID text).
    pub device: String,
    /// `"you@dev"` / `"agent:name@dev"` / `"external@dev"`.
    pub principal: String,
    /// The store's kind tag: `insert`, `set_field`, `edit_text`, `move`, ...
    pub kind: String,
    /// The task this op touched, when any.
    pub task_id: Option<TaskId>,
    /// A one-line human summary, daemon-truncated.
    pub summary: String,
}

/// One synced document (`todo_list`'s implicit file listing, and the `todotxt://` resource list).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileMeta {
    /// Workspace-relative path.
    pub path: RefPath,
    /// `"todo"` / `"done"` / `"notes"`.
    pub kind: &'static str,
}

/// The seam every MCP tool/resource/prompt calls through (mcp-server-tools notes.md). Schemas
/// (`schema.rs`) and transports (`transport.rs`) hold no daemon knowledge of their own — every
/// method here maps to an existing daemon gRPC RPC (see `grpc_backend.rs`), so this crate needs no
/// new daemon surface for the tool table (mcp-server-tools notes.md's audit finding).
#[async_trait::async_trait]
pub trait McpBackend: Send + Sync {
    /// `todo_list`.
    async fn list(&self, args: ListArgs) -> Result<Vec<TaskRow>, McpError>;
    /// `todo_search`.
    async fn search(&self, text: String, file: Option<RefPath>) -> Result<Vec<TaskRow>, McpError>;
    /// `todo_get`.
    async fn get(&self, target: GetTarget) -> Result<TaskRow, McpError>;
    /// `todo_add`.
    async fn add(&self, text: String, file: Option<RefPath>) -> Result<TaskRow, McpError>;
    /// `todo_complete` (`done: true`) / `todo_uncomplete` (`done: false`).
    async fn complete(&self, id: TaskId, done: bool) -> Result<TaskRow, McpError>;
    /// `todo_edit`.
    async fn edit(&self, id: TaskId, patch: FieldPatch) -> Result<TaskRow, McpError>;
    /// `todo_move`.
    async fn move_task(&self, id: TaskId, anchor: MoveAnchor) -> Result<TaskRow, McpError>;
    /// `todo_delete`. `confirm` is asserted by the caller (`tools.rs`) before this is reached.
    async fn delete(&self, id: TaskId, confirm: bool) -> Result<(), McpError>;
    /// `todo_archive`: moves every completed task in `file` to the bottom of the same file, via
    /// one `Apply(MoveToEnd×N)` call. Unlike the CLI's local `archive` (which also collapses the
    /// blank lines left behind — see `daemon_mode.rs`'s own comment on why that specific cleanup
    /// has no clean intent-level mutation), this does not additionally blank-collapse; see the
    /// crate's "As built" notes.
    async fn archive(&self, file: RefPath) -> Result<ApplyOutcome, McpError>;
    /// `todo_batch`. `dry_run: true` is accepted but never calls `Apply` — [mcp-batch-dry-run]
    /// (../../../tasks/mcp-batch-dry-run/notes.md) owns diff rendering, so a dry run here reports
    /// `applied: 0` and no diff rather than pretending to preview one.
    async fn batch(&self, ops: Vec<TodoOp>, dry_run: bool) -> Result<ApplyOutcome, McpError>;
    /// `todo_history`.
    async fn history(
        &self,
        since: Option<Hlc>,
        id: Option<TaskId>,
        file: Option<RefPath>,
    ) -> Result<Vec<OpSummary>, McpError>;
    /// `todo_raw`, read mode.
    async fn raw_read(&self, file: RefPath, lines: Vec<u32>) -> Result<Vec<String>, McpError>;
    /// `todo_raw`, write mode.
    async fn raw_write(&self, file: RefPath, line: u32, text: String) -> Result<(), McpError>;
    /// `todo_notes_get` → daemon gRPC `GetNotes` (plan M5).
    async fn notes_get(&self, id: TaskId) -> Result<String, McpError>;
    /// `todo_notes_set` → daemon gRPC `EditNotes` (plan M5).
    async fn notes_set(&self, id: TaskId, text: String) -> Result<(), McpError>;
    /// Every synced document (the `todotxt://` resource list; not in notes.md's trait sketch, see
    /// the module doc).
    async fn list_files(&self) -> Result<Vec<FileMeta>, McpError>;
    /// A whole file's bytes as text (`todotxt://todo.txt`; see module doc).
    async fn get_file(&self, file: RefPath) -> Result<String, McpError>;
    /// Non-secret identifier for whoever is driving this call: `"agent:<name>#<token_id>"` when a
    /// token principal was attached ([`crate::grpc_backend::GrpcMcpBackend::connect_unix`]'s
    /// `agent` arg), else `"user"` (an unauthenticated stdio session — today's default). Never the
    /// bearer secret, which `token_id` is not: it is a `TokenId` ULID
    /// (`txtodo-daemon/src/tokens.rs::parse_token_id`), the same stable identifier
    /// `Store::verify_token` looks up by, not the once-shown-at-creation bearer string. Used only
    /// by the `mcp.call{tool,principal}` span (plan §5, `txtodo-implementation-plan.md:447`).
    fn principal(&self) -> String;
}
