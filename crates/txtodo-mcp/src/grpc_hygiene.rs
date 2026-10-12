//! `todo_lint` and `todo_conflicts_list`: thin wrappers over the daemon's `Lint` and
//! `ListConflicts` RPCs — the same ones `txtodo lint` and `txtodo conflicts` reach (tasks
//! `mcp-hygiene-parity`, `mcp-conflicts-parity`). `todo_conflicts_resolve` went with same-word
//! review (ADR 0038); a duplicate copy goes with `todo_delete`. Split out of
//! `grpc_read.rs`/`grpc_write.rs` for their file budgets.
//!
//! `todo_fmt` is deliberately not here: it is the one command that rewrites lines it was not asked
//! about, and it stays held until agent-principal attribution covers a whole-file rewrite.

use tonic::transport::Channel;
use txtodo_proto::v1 as pb;
use txtodo_proto::v1::txtodo_client::TxtodoClient;

use crate::backend::{DuplicateCopy, DuplicateGroup, LintFinding, RefPath, WorkspaceArg};
use crate::error::McpError;
use crate::grpc_convert::workspace_selector;
use crate::grpc_read::file_or_root;
// One mapping for every daemon failure, metadata included (task mcp-refusal-metadata).
use crate::grpc_write::status;

/// `todo_lint`.
pub async fn lint(
    mut client: TxtodoClient<Channel>,
    file: Option<RefPath>,
    workspace: WorkspaceArg,
) -> Result<Vec<LintFinding>, McpError> {
    let path = file_or_root(client.clone(), file, &workspace).await?;
    let rep = client
        .lint(pb::LintRequest {
            path,
            workspace: workspace_selector(workspace),
        })
        .await
        .map_err(status)?;
    Ok(rep
        .into_inner()
        .findings
        .into_iter()
        .map(|f| LintFinding {
            line: f.line,
            finding: f.message,
        })
        .collect())
}

/// `todo_conflicts_list`: the document's duplicate groups (ADR 0032), as the daemon derives them.
pub async fn conflicts_list(
    mut client: TxtodoClient<Channel>,
    file: Option<RefPath>,
    workspace: WorkspaceArg,
) -> Result<Vec<DuplicateGroup>, McpError> {
    let path = file_or_root(client.clone(), file, &workspace).await?;
    let rep = client
        .list_conflicts(pb::ConflictsRequest {
            path,
            workspace: workspace_selector(workspace),
        })
        .await
        .map_err(status)?;
    Ok(rep
        .into_inner()
        .duplicates
        .into_iter()
        .map(|g| DuplicateGroup {
            copies: g
                .tasks
                .into_iter()
                .map(|t| DuplicateCopy {
                    id: t.task_id,
                    line: t.line_number,
                })
                .collect(),
        })
        .collect())
}
