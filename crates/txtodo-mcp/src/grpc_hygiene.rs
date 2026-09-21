//! `todo_lint`, `todo_conflicts_list` and `todo_conflicts_resolve`: thin wrappers over the daemon's
//! `Lint`, `ListConflicts` and `ResolveConflict` RPCs — the same ones `txtodo lint` and `txtodo
//! conflicts` reach (tasks `mcp-hygiene-parity`, `mcp-conflicts-parity`). Split out of
//! `grpc_read.rs`/`grpc_write.rs` for their file budgets.
//!
//! `todo_fmt` is deliberately not here: it is the one command that rewrites lines it was not asked
//! about, and it stays held until agent-principal attribution covers a whole-file rewrite.

use tonic::transport::Channel;
use txtodo_proto::v1 as pb;
use txtodo_proto::v1::txtodo_client::TxtodoClient;

use crate::backend::{
    ApplyOutcome, ConflictFlag, ConflictSide, Hlc, LintFinding, RefPath, TaskId, WorkspaceArg,
};
use crate::error::McpError;
use crate::grpc_convert::{hex, workspace_selector};
use crate::grpc_read::{file_or_root, locate_by_id};
use crate::grpc_write::GrpcCtx;

fn status(s: tonic::Status) -> McpError {
    McpError::daemon(s.message().to_owned())
}

/// `todo_lint`.
pub async fn lint(
    mut client: TxtodoClient<Channel>,
    file: Option<RefPath>,
    workspace: WorkspaceArg,
) -> Result<Vec<LintFinding>, McpError> {
    let path = file_or_root(client.clone(), file, &workspace).await;
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

/// The open flags of one document, as the daemon reports them.
async fn flags(
    client: &mut TxtodoClient<Channel>,
    path: &str,
    workspace: WorkspaceArg,
) -> Result<Vec<pb::ReviewFlag>, McpError> {
    let rep = client
        .list_conflicts(pb::ConflictsRequest {
            path: path.to_owned(),
            workspace: workspace_selector(workspace),
        })
        .await
        .map_err(status)?;
    Ok(rep.into_inner().flags)
}

/// `todo_conflicts_list`.
pub async fn conflicts_list(
    mut client: TxtodoClient<Channel>,
    file: Option<RefPath>,
    workspace: WorkspaceArg,
) -> Result<Vec<ConflictFlag>, McpError> {
    let path = file_or_root(client.clone(), file, &workspace).await;
    Ok(flags(&mut client, &path, workspace)
        .await?
        .into_iter()
        .map(|f| ConflictFlag {
            id: f.task_id,
            line: f.line_number,
            mine: f.mine,
            theirs: f.theirs,
            raised_at_ms: f.raised_at_ms,
        })
        .collect())
}

fn wire(side: ConflictSide) -> pb::Resolution {
    match side {
        ConflictSide::Mine => pb::Resolution::Mine,
        ConflictSide::Theirs => pb::Resolution::Theirs,
        ConflictSide::Merged => pb::Resolution::Merged,
    }
}

/// `todo_conflicts_resolve`. The daemon addresses a task by line as well as id, and the line a
/// flagged task sits on is in its flag, so nothing is guessed: no open flag for `id` is `not_found`.
pub async fn conflicts_resolve(
    ctx: GrpcCtx,
    id: TaskId,
    side: ConflictSide,
    file: Option<RefPath>,
    workspace: WorkspaceArg,
) -> Result<ApplyOutcome, McpError> {
    let mut client = ctx.client.clone();
    let path = match file {
        Some(path) => path,
        None => {
            locate_by_id(client.clone(), &id, workspace.clone())
                .await?
                .0
        }
    };
    let flag = flags(&mut client, &path, workspace.clone())
        .await?
        .into_iter()
        .find(|f| f.task_id == id)
        .ok_or_else(|| McpError::not_found(format!("no open conflict for {id} in {path}")))?;
    let applied = client
        .resolve_conflict(pb::ResolveRequest {
            path,
            task: Some(pb::TaskRef {
                line_number: flag.line_number,
                task_id: id,
            }),
            resolution: wire(side) as i32,
            workspace: workspace_selector(workspace),
            agent: ctx.agent,
        })
        .await
        .map_err(status)?
        .into_inner();
    Ok(ApplyOutcome {
        applied: applied.applied,
        hash: (!applied.hash.is_empty()).then(|| hex(&applied.hash)),
        hlc: Some(Hlc {
            wall_ms: applied.hlc_wall_ms,
            counter: applied.hlc_counter,
        }),
        diff: None,
    })
}
