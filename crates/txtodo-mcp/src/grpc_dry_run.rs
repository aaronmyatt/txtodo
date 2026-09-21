//! `todo_batch` with `dry_run` (task apply-dry-run): the daemon plans the batch through its real
//! mutation path and returns the unified diff it would make; nothing is written. Each op is turned
//! into the mutation its single-tool counterpart would send, addressed by task id alone (line 0
//! plus the id) because earlier ops in the batch move lines, and the mutations are grouped per file
//! into one dry-run `Apply` each.
//!
//! Known limits: every op is read against the document as it is now, so two ops on the same task
//! (complete it, then edit it) preview the edit from the pre-batch text; and `todo_move` cannot be
//! previewed yet, so a batch holding one is refused rather than half shown.

use crate::backend::{ApplyOutcome, RefPath, TaskId, TodoOp, WorkspaceArg};
use crate::error::McpError;
use crate::grpc_convert::workspace_selector;
use crate::grpc_read::{file_or_root, locate_by_id};
use crate::grpc_write::{GrpcCtx, apply_patch, status, validate_add_text};
use txtodo_proto::v1 as pb;

/// `id` as a `TaskRef` that names the task by id alone.
fn by_id(id: TaskId) -> Option<pb::TaskRef> {
    Some(pb::TaskRef {
        line_number: 0,
        task_id: id,
    })
}

fn mutation(kind: pb::mutation::Kind) -> pb::Mutation {
    pb::Mutation { kind: Some(kind) }
}

/// The file and mutation `op` would send, or `None` when it would change nothing (completing a task
/// that is done already, an edit that leaves the line as it is).
async fn planned(
    ctx: &GrpcCtx,
    op: TodoOp,
    workspace: &WorkspaceArg,
) -> Result<Option<(RefPath, pb::Mutation)>, McpError> {
    let locate = |id: TaskId| {
        let (client, workspace) = (ctx.client.clone(), workspace.clone());
        async move { locate_by_id(client, &id, workspace).await }
    };
    Ok(match op {
        TodoOp::TodoAdd { text, file } => Some(add_plan(ctx, text, file, workspace).await?),
        TodoOp::TodoComplete { id } => {
            let (path, _, row) = locate(id.clone()).await?;
            (!row.done).then(|| {
                let done = pb::Complete {
                    task: by_id(id),
                    today: crate::grpc_write::today_local(),
                };
                (path, mutation(pb::mutation::Kind::Complete(done)))
            })
        }
        TodoOp::TodoUncomplete { id } => {
            let (path, _, row) = locate(id.clone()).await?;
            row.done.then(|| {
                let reopen = pb::Reopen { task: by_id(id) };
                (path, mutation(pb::mutation::Kind::Reopen(reopen)))
            })
        }
        TodoOp::TodoEdit { id, patch } => {
            let (path, _, row) = locate(id.clone()).await?;
            let new_line = apply_patch(&row.raw, &patch);
            (new_line != row.raw).then(|| {
                let edit = pb::Edit {
                    task: by_id(id),
                    new_line,
                };
                (path, mutation(pb::mutation::Kind::Edit(edit)))
            })
        }
        TodoOp::TodoDelete { id, confirm } => {
            if !confirm {
                return Err(McpError::confirm_required(
                    "todo_delete needs confirm: true",
                ));
            }
            let (path, _, _) = locate(id.clone()).await?;
            let delete = pb::Delete {
                task: by_id(id),
                leave_blank: true,
            };
            Some((path, mutation(pb::mutation::Kind::Delete(delete))))
        }
        TodoOp::TodoMove { .. } => {
            return Err(McpError::invalid_params(
                "todo_move cannot be previewed with dry_run yet; run the batch without it",
            ));
        }
    })
}

/// The unified diff `ops` would make, one dry-run `Apply` per file, in the order the files first
/// appear. The outcome has no hash or clock stamp: nothing was written.
pub(crate) async fn preview(
    ctx: GrpcCtx,
    ops: Vec<TodoOp>,
    workspace: WorkspaceArg,
) -> Result<ApplyOutcome, McpError> {
    let mut files: Vec<(RefPath, Vec<pb::Mutation>)> = Vec::new();
    for op in ops {
        let Some((path, m)) = planned(&ctx, op, &workspace).await? else {
            continue;
        };
        match files.iter_mut().find(|(p, _)| *p == path) {
            Some((_, mutations)) => mutations.push(m),
            None => files.push((path, vec![m])),
        }
    }
    let (mut applied, mut diff) = (0u32, String::new());
    for (path, mutations) in files {
        let req = pb::ApplyRequest {
            path,
            mutations,
            agent: ctx.agent.clone(),
            workspace: workspace_selector(workspace.clone()),
            source: "mcp".to_owned(),
            dry_run: true,
        };
        let rep = ctx
            .client
            .clone()
            .apply(req)
            .await
            .map_err(status)?
            .into_inner();
        applied += rep.applied;
        diff.push_str(&rep.diff);
    }
    Ok(ApplyOutcome {
        applied,
        hash: None,
        hlc: None,
        diff: (!diff.is_empty()).then_some(diff),
    })
}

/// An add needs no lookup of the line: the file (default the root list) and the line as the tool
/// would send it.
async fn add_plan(
    ctx: &GrpcCtx,
    text: String,
    file: Option<RefPath>,
    workspace: &WorkspaceArg,
) -> Result<(RefPath, pb::Mutation), McpError> {
    validate_add_text(&text)?;
    let path = file_or_root(ctx.client.clone(), file, workspace).await;
    Ok((
        path,
        mutation(pb::mutation::Kind::Add(pb::Add { line: text })),
    ))
}
