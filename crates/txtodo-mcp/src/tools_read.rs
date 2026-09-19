//! Bodies for the read-only tools (`todo_list`, `todo_search`, `todo_get`, `todo_history`,
//! `todo_raw` read mode). Called one-line-each from `schema.rs`'s `#[tool_router]` impl.

use rmcp::ErrorData;
use rmcp::model::CallToolResult;

use crate::backend::{
    ConflictsListArgs, GetTarget, HistoryArgs, LintArgs, ListArgs, McpBackend, RawArgs, SearchArgs,
    WorkspaceArg,
};
use crate::error::McpError;
use crate::tools::json_result;

/// `todo_list`.
pub async fn list(backend: &dyn McpBackend, args: ListArgs) -> Result<CallToolResult, ErrorData> {
    json_result(&backend.list(args).await?)
}

/// `todo_search`.
pub async fn search(
    backend: &dyn McpBackend,
    args: SearchArgs,
) -> Result<CallToolResult, ErrorData> {
    json_result(&backend.search(args.text, args.file, args.workspace).await?)
}

/// `todo_get`.
pub async fn get(backend: &dyn McpBackend, target: GetTarget) -> Result<CallToolResult, ErrorData> {
    if target.id.is_none() && target.line.is_none() {
        return Err(McpError::invalid_params("todo_get needs id or line").into());
    }
    json_result(&backend.get(target).await?)
}

/// `todo_lint`: read-only findings, the same a `txtodo lint` prints.
pub async fn lint(backend: &dyn McpBackend, args: LintArgs) -> Result<CallToolResult, ErrorData> {
    json_result(&backend.lint(args.file, args.workspace).await?)
}

/// `todo_conflicts_list`.
pub async fn conflicts_list(
    backend: &dyn McpBackend,
    args: ConflictsListArgs,
) -> Result<CallToolResult, ErrorData> {
    json_result(&backend.conflicts_list(args.file, args.workspace).await?)
}

/// `todo_history`.
pub async fn history(
    backend: &dyn McpBackend,
    args: HistoryArgs,
) -> Result<CallToolResult, ErrorData> {
    let ops = backend
        .history(args.since, args.id, args.file, args.workspace)
        .await?;
    json_result(&ops)
}

/// `todo_raw`, both modes: `text` present means write, otherwise read (`args.rs`'s doc).
pub async fn raw(backend: &dyn McpBackend, args: RawArgs) -> Result<CallToolResult, ErrorData> {
    let RawArgs {
        file,
        lines,
        line,
        text,
        workspace,
    } = args;
    match text {
        Some(text) => {
            let line =
                line.ok_or_else(|| McpError::invalid_params("todo_raw write mode needs line"))?;
            backend.raw_write(file, line, text, workspace).await?;
            json_result(&())
        }
        None => {
            let lines =
                lines.ok_or_else(|| McpError::invalid_params("todo_raw read mode needs lines"))?;
            json_result(&backend.raw_read(file, lines, workspace).await?)
        }
    }
}

/// `todo_notes_get`.
pub async fn notes_get(
    backend: &dyn McpBackend,
    id: String,
    workspace: WorkspaceArg,
) -> Result<CallToolResult, ErrorData> {
    json_result(&backend.notes_get(id, workspace).await?)
}
