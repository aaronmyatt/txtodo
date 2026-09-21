//! Bodies for the mutating tools (`todo_add` through `todo_batch`, `todo_notes_set`). Called
//! one-line-each from `schema.rs`'s `#[tool_router]` impl.

use rmcp::ErrorData;
use rmcp::model::CallToolResult;

use crate::backend::{
    AddArgs, ArchiveArgs, BatchArgs, ConflictsResolveArgs, DeleteArgs, EditArgs, McpBackend,
    MoveArgs, WorkspaceArg, move_anchor,
};
use crate::error::McpError;
use crate::tools::json_result;

/// `todo_add`.
pub async fn add(backend: &dyn McpBackend, args: AddArgs) -> Result<CallToolResult, ErrorData> {
    json_result(&backend.add(args.text, args.file, args.workspace).await?)
}

/// `todo_complete` / `todo_uncomplete`: `done` is fixed by which tool called this.
pub async fn complete(
    backend: &dyn McpBackend,
    id: String,
    done: bool,
    workspace: WorkspaceArg,
) -> Result<CallToolResult, ErrorData> {
    json_result(&backend.complete(id, done, workspace).await?)
}

/// `todo_edit`.
pub async fn edit(backend: &dyn McpBackend, args: EditArgs) -> Result<CallToolResult, ErrorData> {
    json_result(&backend.edit(args.id, args.patch, args.workspace).await?)
}

/// `todo_move`.
pub async fn move_task(
    backend: &dyn McpBackend,
    args: MoveArgs,
) -> Result<CallToolResult, ErrorData> {
    let anchor = move_anchor(args.before, args.after)?;
    json_result(&backend.move_task(args.id, anchor, args.workspace).await?)
}

/// `todo_conflicts_resolve`: attributed to this session's agent (`ResolveRequest.agent`).
pub async fn conflicts_resolve(
    backend: &dyn McpBackend,
    args: ConflictsResolveArgs,
) -> Result<CallToolResult, ErrorData> {
    json_result(
        &backend
            .conflicts_resolve(args.id, args.side, args.file, args.workspace)
            .await?,
    )
}

/// `todo_delete`. `confirm` is asserted here — before the backend is even called — not merely
/// forwarded (design §6.3 invariant: "assert the arg, don't trust").
pub async fn delete(
    backend: &dyn McpBackend,
    args: DeleteArgs,
) -> Result<CallToolResult, ErrorData> {
    if !args.confirm {
        return Err(McpError::confirm_required("todo_delete needs confirm: true").into());
    }
    backend
        .delete(args.id, args.confirm, args.workspace)
        .await?;
    json_result(&())
}

/// `todo_archive`.
pub async fn archive(
    backend: &dyn McpBackend,
    args: ArchiveArgs,
) -> Result<CallToolResult, ErrorData> {
    json_result(&backend.archive(args.file, args.workspace).await?)
}

/// `todo_batch`.
pub async fn batch(backend: &dyn McpBackend, args: BatchArgs) -> Result<CallToolResult, ErrorData> {
    json_result(
        &backend
            .batch(args.ops, args.dry_run, args.workspace)
            .await?,
    )
}

/// `todo_notes_set`.
pub async fn notes_set(
    backend: &dyn McpBackend,
    id: String,
    text: String,
    workspace: WorkspaceArg,
) -> Result<CallToolResult, ErrorData> {
    backend.notes_set(id, text, workspace).await?;
    json_result(&())
}
