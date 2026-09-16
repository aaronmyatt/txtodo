//! The [`McpBackend`] trait (mcp-server-tools notes.md "Design"): schemas and transports
//! (`schema.rs`/`transport.rs`) never touch the daemon directly, only through this seam. The
//! concrete implementation ([`crate::grpc_backend::GrpcMcpBackend`]) calls the existing
//! `Apply`/`GetFile`/`ListFiles`/`History`/`GetNotes`/`EditNotes`/`WorkspaceList` daemon RPCs.
//!
//! Two methods here — `list_files`/`get_file` — are not in the notes.md sketch of this trait: they
//! back the `todotxt://todo.txt` resource and the file list, both needing a whole file's bytes
//! rather than a single task. Both are thin wrappers over the daemon's existing `ListFiles`/
//! `GetFile` RPCs (no new daemon surface), so they stay in scope for this task. Every tool
//! argument struct lives in `backend_args.rs` (split out for the file budget) and is re-exported
//! below so `crate::backend::<Type>` keeps working at every existing call site.

use serde::{Deserialize, Serialize};

pub use crate::backend_args::*;
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

/// One entry in the device-global workspace registry (`WorkspaceList` RPC; `todo_list_workspaces`
/// resource). Mirrors `txtodo_proto::v1::WorkspaceInfo` field for field.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceInfo {
    /// `WorkspaceId` ULID text — the value a `workspace` arg accepts as an id.
    pub id: String,
    /// Canonicalized absolute path — the value a `workspace` arg accepts as a path.
    pub root: String,
    /// Unix milliseconds this workspace was first registered.
    pub added_at_ms: u64,
    /// Cheap `fs::exists` check; `false` means the directory moved or was deleted.
    pub root_exists: bool,
    /// Whether `root/.txtodo/oplog.db` exists; `false` means never opened yet.
    pub has_state: bool,
}

/// The seam every MCP tool/resource/prompt calls through (mcp-server-tools notes.md). Schemas
/// (`schema.rs`) and transports (`transport.rs`) hold no daemon knowledge of their own — every
/// method here maps to an existing daemon gRPC RPC (see `grpc_backend.rs`), so this crate needs no
/// new daemon surface for the tool table (mcp-server-tools notes.md's audit finding). Every method
/// below except `list_workspaces`/`principal` takes a `workspace: WorkspaceArg` (or reads
/// `args.workspace`, when the call already takes a struct) — the daemon's own `WorkspaceSelector`,
/// threaded through (`backend_args.rs`'s module doc, mcp-multi-workspace-gateway notes.md).
#[async_trait::async_trait]
pub trait McpBackend: Send + Sync {
    /// `todo_list`.
    async fn list(&self, args: ListArgs) -> Result<Vec<TaskRow>, McpError>;
    /// `todo_search`.
    async fn search(
        &self,
        text: String,
        file: Option<RefPath>,
        workspace: WorkspaceArg,
    ) -> Result<Vec<TaskRow>, McpError>;
    /// `todo_get`.
    async fn get(&self, target: GetTarget) -> Result<TaskRow, McpError>;
    /// `todo_add`.
    async fn add(
        &self,
        text: String,
        file: Option<RefPath>,
        workspace: WorkspaceArg,
    ) -> Result<TaskRow, McpError>;
    /// `todo_complete` (`done: true`) / `todo_uncomplete` (`done: false`).
    async fn complete(
        &self,
        id: TaskId,
        done: bool,
        workspace: WorkspaceArg,
    ) -> Result<TaskRow, McpError>;
    /// `todo_edit`.
    async fn edit(
        &self,
        id: TaskId,
        patch: FieldPatch,
        workspace: WorkspaceArg,
    ) -> Result<TaskRow, McpError>;
    /// `todo_move`.
    async fn move_task(
        &self,
        id: TaskId,
        anchor: MoveAnchor,
        workspace: WorkspaceArg,
    ) -> Result<TaskRow, McpError>;
    /// `todo_delete`. `confirm` is asserted by the caller (`tools.rs`) before this is reached.
    async fn delete(
        &self,
        id: TaskId,
        confirm: bool,
        workspace: WorkspaceArg,
    ) -> Result<(), McpError>;
    /// `todo_archive`: moves every completed task in `file` to the bottom of the same file, via
    /// one `Apply(MoveToEnd×N)` call. Unlike the CLI's local `archive` (which also collapses the
    /// blank lines left behind — see `daemon_mode.rs`'s own comment on why that specific cleanup
    /// has no clean intent-level mutation), this does not additionally blank-collapse; see the
    /// crate's "As built" notes.
    async fn archive(
        &self,
        file: RefPath,
        workspace: WorkspaceArg,
    ) -> Result<ApplyOutcome, McpError>;
    /// `todo_batch`. `dry_run: true` is accepted but never calls `Apply` — [mcp-batch-dry-run]
    /// (../../../tasks/mcp-batch-dry-run/notes.md) owns diff rendering, so a dry run here reports
    /// `applied: 0` and no diff rather than pretending to preview one. `workspace` applies to
    /// every op in `ops` (`BatchArgs`'s own doc).
    async fn batch(
        &self,
        ops: Vec<TodoOp>,
        dry_run: bool,
        workspace: WorkspaceArg,
    ) -> Result<ApplyOutcome, McpError>;
    /// `todo_history`.
    async fn history(
        &self,
        since: Option<Hlc>,
        id: Option<TaskId>,
        file: Option<RefPath>,
        workspace: WorkspaceArg,
    ) -> Result<Vec<OpSummary>, McpError>;
    /// `todo_raw`, read mode.
    async fn raw_read(
        &self,
        file: RefPath,
        lines: Vec<u32>,
        workspace: WorkspaceArg,
    ) -> Result<Vec<String>, McpError>;
    /// `todo_raw`, write mode.
    async fn raw_write(
        &self,
        file: RefPath,
        line: u32,
        text: String,
        workspace: WorkspaceArg,
    ) -> Result<(), McpError>;
    /// `todo_notes_get` → daemon gRPC `GetNotes` (plan M5).
    async fn notes_get(&self, id: TaskId, workspace: WorkspaceArg) -> Result<String, McpError>;
    /// `todo_notes_set` → daemon gRPC `EditNotes` (plan M5).
    async fn notes_set(
        &self,
        id: TaskId,
        text: String,
        workspace: WorkspaceArg,
    ) -> Result<(), McpError>;
    /// Every synced document (the `todotxt://` resource list; not in notes.md's trait sketch, see
    /// the module doc).
    async fn list_files(&self, workspace: WorkspaceArg) -> Result<Vec<FileMeta>, McpError>;
    /// A whole file's bytes as text (`todotxt://todo.txt`; see module doc).
    async fn get_file(&self, file: RefPath, workspace: WorkspaceArg) -> Result<String, McpError>;
    /// `todo_list_workspaces` resource → daemon gRPC `WorkspaceList`. Device-global, unscoped by
    /// any `WorkspaceSelector` — the registry listing is not itself one of the workspaces it lists.
    async fn list_workspaces(&self) -> Result<Vec<WorkspaceInfo>, McpError>;
    /// Non-secret identifier for whoever is driving this call: `"agent:<name>#<token_id>"` when a
    /// token principal was attached ([`crate::grpc_backend::GrpcMcpBackend::connect_unix`]'s
    /// `agent` arg), else `"user"` (an unauthenticated stdio session — today's default). Never the
    /// bearer secret, which `token_id` is not: it is a `TokenId` ULID
    /// (`txtodo-daemon/src/tokens.rs::parse_token_id`), the same stable identifier
    /// `Store::verify_token` looks up by, not the once-shown-at-creation bearer string. Used only
    /// by the `mcp.call{tool,principal}` span (plan §5, `txtodo-implementation-plan.md:447`).
    fn principal(&self) -> String;
}
