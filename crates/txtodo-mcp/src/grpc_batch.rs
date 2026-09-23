//! `todo_batch` (`batch`, `apply_planned`, `apply_batch_op`), split out of `grpc_write.rs` for
//! that file's line budget.

use crate::backend::{ApplyOutcome, TodoOp, WorkspaceArg, move_anchor};
use crate::error::McpError;
use crate::grpc_write::{GrpcCtx, add, complete, delete, edit, status};
use txtodo_proto::v1 as pb;

/// `todo_batch`. `dry_run: true` asks the daemon for the diff (`grpc_dry_run.rs`). A real run reuses
/// each op's single-tool counterpart above, sequentially: it is not one atomic multi-mutation
/// `Apply`, since ops may target different files and `apply_route.rs` never allows a `Move`
/// alongside anything else in one batch. `workspace` applies to every op.
pub async fn batch(
    ctx: GrpcCtx,
    ops: Vec<TodoOp>,
    dry_run: bool,
    workspace: WorkspaceArg,
) -> Result<ApplyOutcome, McpError> {
    if dry_run {
        return crate::grpc_dry_run::preview(ctx, ops, workspace).await;
    }
    // The same grouped-by-id plan the preview builds, so a dry run's diff is the run's write
    // (task batch-dry-run-divergence) and the batch is one commit per file, not one per op. A
    // batch the plan cannot express (a task named twice, a todo_move) takes the per-op path,
    // which re-reads the document between ops.
    if crate::grpc_dry_run::plannable(&ops) {
        return apply_planned(ctx, ops, workspace).await;
    }
    let mut applied = 0u32;
    for op in ops {
        apply_batch_op(ctx.clone(), op, workspace.clone()).await?;
        applied += 1;
    }
    Ok(ApplyOutcome {
        applied,
        ..ApplyOutcome::default()
    })
}

/// One real `Apply` per file from the shared plan; the outcome carries the last file's hash and
/// clock stamp and the total ops applied.
async fn apply_planned(
    ctx: GrpcCtx,
    ops: Vec<TodoOp>,
    workspace: WorkspaceArg,
) -> Result<ApplyOutcome, McpError> {
    let files = crate::grpc_dry_run::plan_files(&ctx, ops, &workspace).await?;
    let mut outcome = ApplyOutcome::default();
    for (path, mutations) in files {
        let req = pb::ApplyRequest {
            path,
            mutations,
            agent: ctx.agent.clone(),
            workspace: crate::grpc_convert::workspace_selector(workspace.clone()),
            source: "mcp".to_owned(),
            dry_run: false,
        };
        let rep = ctx
            .client
            .clone()
            .apply(req)
            .await
            .map_err(status)?
            .into_inner();
        outcome.applied += rep.applied;
        outcome.hash = Some(crate::grpc_convert::hex(&rep.hash));
        outcome.hlc = Some(crate::backend::Hlc {
            wall_ms: rep.hlc_wall_ms,
            counter: rep.hlc_counter,
        });
    }
    Ok(outcome)
}

async fn apply_batch_op(ctx: GrpcCtx, op: TodoOp, workspace: WorkspaceArg) -> Result<(), McpError> {
    match op {
        TodoOp::TodoAdd { text, file } => {
            add(ctx, text, file, workspace).await?;
        }
        TodoOp::TodoComplete { id } => {
            complete(ctx, id, true, workspace).await?;
        }
        TodoOp::TodoUncomplete { id } => {
            complete(ctx, id, false, workspace).await?;
        }
        TodoOp::TodoEdit { id, patch } => {
            edit(ctx, id, patch, workspace).await?;
        }
        TodoOp::TodoMove { id, before, after } => {
            crate::grpc_move::move_task(ctx, id, move_anchor(before, after)?, workspace).await?;
        }
        TodoOp::TodoDelete { id, confirm } => {
            delete(ctx, id, confirm, workspace).await?;
        }
    }
    Ok(())
}
