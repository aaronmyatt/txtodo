//! Write-side [`crate::backend::McpBackend`] logic. Split out of `grpc_backend.rs` for the file
//! budget. Every function goes through [`apply_one`], one `Apply` RPC per call — batching several
//! mutations atomically only works for a single document and (per `apply_route.rs`) never mixes a
//! `Move` with anything else, so `archive`/`batch` below issue one `Apply` per task instead.

use tonic::transport::Channel;
use txtodo_proto::v1 as pb;
use txtodo_proto::v1::txtodo_client::TxtodoClient;

use crate::backend::{ApplyOutcome, FieldPatch, Hlc, RefPath, TaskId, TaskRow, WorkspaceArg};
use crate::error::McpError;
use crate::grpc_convert::{hex, workspace_selector};
use crate::grpc_read::{file_or_root, get_file_doc, locate_by_id};
use crate::parse;

/// The gRPC channel plus the principal every mutation is stamped with (design §6.2's `agent`
/// field; `None` until [mcp-agent-principal](../../../tasks/mcp-agent-principal) fills it in from
/// a real token).
#[derive(Clone)]
pub struct GrpcCtx {
    /// The connected client. `TxtodoClient<Channel>` clones cheaply (`Channel` is `Arc`-backed);
    /// every call below clones its own rather than sharing a lock.
    pub client: TxtodoClient<Channel>,
    /// Attached to every `Apply` call as `ApplyRequest.agent`.
    pub agent: Option<pb::AgentPrincipal>,
}

/// The daemon's failure as an `McpError`, with the line and spec rule it sent as metadata (task
/// apply-dry-run: structured errors) passed through verbatim — the daemon owns the rule list
/// (`MutationError::spec_rule`), so nothing here allow-lists it (task mcp-refusal-metadata).
/// The keys are `txtodo-daemon`'s `ERROR_LINE_KEY` and `ERROR_RULE_KEY`; this crate may not
/// depend on it, so the strings are repeated here. The one `Status` → `McpError` mapping every
/// gRPC body in this crate uses, reads and notes included.
pub(crate) fn status(s: tonic::Status) -> McpError {
    let mut error = McpError::daemon(s.message().to_owned());
    let meta = |key: &str| s.metadata().get(key).and_then(|v| v.to_str().ok());
    if let Some(line) = meta("x-txtodo-error-line").and_then(|v| v.parse().ok()) {
        error = error.with_line(line);
    }
    if let Some(rule) = meta("x-txtodo-error-rule") {
        error = error.with_spec_rule(rule);
    }
    error
}

/// Today, local date, `YYYY-MM-DD` (ADR 0011: local date, never a time zone) — the same source
/// `txtodo-cli`'s `clock.rs::today_local` uses, reimplemented here for the crate-boundary reason
/// explained in `parse.rs`'s module doc (that crate is a binary, not a library).
pub fn today_local() -> String {
    jiff::Zoned::now().strftime("%Y-%m-%d").to_string()
}

pub(crate) async fn apply_one(
    ctx: GrpcCtx,
    path: &str,
    mutation: pb::Mutation,
    workspace: WorkspaceArg,
) -> Result<pb::ApplyResponse, McpError> {
    let mut client = ctx.client;
    let req = pb::ApplyRequest {
        path: path.to_owned(),
        mutations: vec![mutation],
        agent: ctx.agent,
        workspace: workspace_selector(workspace),
        // The activity log tells an MCP agent's change from a CLI or desktop one (task op-source).
        source: "mcp".to_owned(),
        dry_run: false,
    };
    let rep = client.apply(req).await.map_err(status)?;
    Ok(rep.into_inner())
}

/// `todo_add`.
pub async fn add(
    ctx: GrpcCtx,
    text: String,
    file: Option<RefPath>,
    workspace: WorkspaceArg,
) -> Result<TaskRow, McpError> {
    validate_add_text(&text)?;
    let path = file_or_root(ctx.client.clone(), file, &workspace).await?;
    let mutation = pb::Mutation {
        kind: Some(pb::mutation::Kind::Add(pb::Add { line: text.clone() })),
    };
    let client = ctx.client.clone();
    apply_one(ctx, &path, mutation, workspace.clone()).await?;
    find_added_row(client, &path, &text, workspace).await
}

/// design §6.3: "`todo_add` text with a hand-written `created:` already present is a client error,
/// not silently re-stamped" — read as "the leading date, or an `id:` tag" (todo.txt has no literal
/// `created:` key; the creation timestamp *is* the leading bare date).
pub(crate) fn validate_add_text(text: &str) -> Result<(), McpError> {
    let (_, date_len) = parse::prefix_lens(text);
    if date_len > 0 {
        return Err(McpError::invalid_params(
            "todo_add text must not include a leading date; the daemon stamps it",
        )
        .with_spec_rule("creation-date"));
    }
    if text.split_whitespace().any(|w| w.starts_with("id:")) {
        return Err(McpError::invalid_params(
            "todo_add text must not include an id: tag; the daemon stamps it",
        )
        .with_spec_rule("id-tag"));
    }
    Ok(())
}

async fn find_added_row(
    client: TxtodoClient<Channel>,
    path: &str,
    text: &str,
    workspace: WorkspaceArg,
) -> Result<TaskRow, McpError> {
    let after = get_file_doc(client, path, workspace).await?;
    parse::lines(&after.text)
        .into_iter()
        .rev()
        .find(|(_, raw)| raw.contains(text.trim()))
        .map(|(n, raw)| after.row(n, raw))
        .ok_or_else(|| McpError::daemon("added task could not be re-read"))
}

/// `todo_complete` (`done: true`) / `todo_uncomplete` (`done: false`).
pub async fn complete(
    ctx: GrpcCtx,
    id: TaskId,
    done: bool,
    workspace: WorkspaceArg,
) -> Result<TaskRow, McpError> {
    let (path, line, row) = locate_by_id(ctx.client.clone(), &id, workspace.clone()).await?;
    if row.done == done {
        return Ok(row);
    }
    let task_ref = pb::TaskRef {
        line_number: line,
        task_id: id.clone(),
    };
    let mutation = if done {
        pb::Mutation {
            kind: Some(pb::mutation::Kind::Complete(pb::Complete {
                task: Some(task_ref),
                today: today_local(),
            })),
        }
    } else {
        // The daemon clears the `x`, restores `(X)` from `pri:X`, and moves the line to the end of
        // the open block (task complete-to-bottom), so an Edit here would leave it in place.
        pb::Mutation {
            kind: Some(pb::mutation::Kind::Reopen(pb::Reopen {
                task: Some(task_ref),
            })),
        }
    };
    let client = ctx.client.clone();
    apply_one(ctx, &path, mutation, workspace.clone()).await?;
    let (_, _, row) = locate_by_id(client, &id, workspace).await?;
    Ok(row)
}

/// `todo_edit`.
pub async fn edit(
    ctx: GrpcCtx,
    id: TaskId,
    patch: FieldPatch,
    workspace: WorkspaceArg,
) -> Result<TaskRow, McpError> {
    let (path, line, row) = locate_by_id(ctx.client.clone(), &id, workspace.clone()).await?;
    let new_line = apply_patch(&row.raw, &patch);
    if new_line == row.raw {
        return Ok(row);
    }
    let task_ref = pb::TaskRef {
        line_number: line,
        task_id: id.clone(),
    };
    let mutation = pb::Mutation {
        kind: Some(pb::mutation::Kind::Edit(pb::Edit {
            task: Some(task_ref),
            new_line,
        })),
    };
    let client = ctx.client.clone();
    apply_one(ctx, &path, mutation, workspace.clone()).await?;
    let (_, _, row) = locate_by_id(client, &id, workspace).await?;
    Ok(row)
}

/// Priority/due first (structural), then append/replace (text-level) last.
pub(crate) fn apply_patch(raw: &str, patch: &FieldPatch) -> String {
    let mut line = raw.to_owned();
    if let Some(p) = &patch.priority {
        line = parse::set_priority(&line, p.chars().next().map(|c| c.to_ascii_uppercase()));
    }
    if let Some(d) = &patch.due {
        line = parse::set_kv(&line, "due", (!d.is_empty()).then_some(d.as_str()));
    }
    if let Some(a) = &patch.append {
        line = parse::append(&line, a);
    }
    if let Some(r) = &patch.replace {
        line = parse::replace_body(&line, r);
    }
    line
}

/// `todo_delete`. `confirm` is asserted, never trusted (design §6.3 invariant).
pub async fn delete(
    ctx: GrpcCtx,
    id: TaskId,
    confirm: bool,
    workspace: WorkspaceArg,
) -> Result<(), McpError> {
    if !confirm {
        return Err(McpError::confirm_required(
            "todo_delete needs confirm: true",
        ));
    }
    let (path, line, _row) = locate_by_id(ctx.client.clone(), &id, workspace.clone()).await?;
    let task_ref = pb::TaskRef {
        line_number: line,
        task_id: id,
    };
    let mutation = pb::Mutation {
        kind: Some(pb::mutation::Kind::Delete(pb::Delete {
            task: Some(task_ref),
            leave_blank: true,
        })),
    };
    apply_one(ctx, &path, mutation, workspace).await?;
    Ok(())
}

/// `todo_archive`: one `Apply(MoveToEnd)` per completed task, in original file order, so a task
/// already pushed to the bottom never has to move again — the daemon computes each task's new
/// position live (after the current last other task in the file), so only the line number for the
/// `TaskRef` needs tracking here as earlier moves close up the gap they leave behind.
pub async fn archive(
    ctx: GrpcCtx,
    file: Option<RefPath>,
    workspace: WorkspaceArg,
) -> Result<ApplyOutcome, McpError> {
    let file = file_or_root(ctx.client.clone(), file, &workspace).await?;
    let doc = get_file_doc(ctx.client.clone(), &file, workspace.clone()).await?;
    let mut completed: Vec<(u32, String)> = doc
        .rows()
        .into_iter()
        .filter(|r| r.done)
        .map(|r| (r.line, r.id.unwrap_or_default()))
        .collect();
    completed.sort_by_key(|(line, _)| *line);
    let mut applied = 0u32;
    let mut last: Option<pb::ApplyResponse> = None;
    for i in 0..completed.len() {
        let (line, task_id) = completed[i].clone();
        let task_ref = pb::TaskRef {
            line_number: line,
            task_id,
        };
        let mutation = pb::Mutation {
            kind: Some(pb::mutation::Kind::MoveToEnd(pb::MoveToEnd {
                task: Some(task_ref),
            })),
        };
        let resp = apply_one(ctx.clone(), &file, mutation, workspace.clone()).await?;
        applied += resp.applied;
        last = Some(resp);
        // The moved line's old slot closes up: every not-yet-processed line after it shifts back one.
        for later in completed.iter_mut().skip(i + 1) {
            if later.0 > line {
                later.0 -= 1;
            }
        }
    }
    Ok(outcome_from(applied, last))
}

fn outcome_from(applied: u32, last: Option<pb::ApplyResponse>) -> ApplyOutcome {
    match last {
        Some(r) => ApplyOutcome {
            applied,
            hash: Some(hex(&r.hash)),
            hlc: Some(Hlc {
                wall_ms: r.hlc_wall_ms,
                counter: r.hlc_counter,
            }),
            diff: None,
        },
        None => ApplyOutcome::default(),
    }
}

/// `todo_raw`, write mode: an `Edit` mutation that bypasses structured parsing entirely (needs the
/// `raw` scope — enforced by [mcp-auth](../../../tasks/mcp-auth), not here).
pub async fn raw_write(
    ctx: GrpcCtx,
    file: RefPath,
    line: u32,
    text: String,
    workspace: WorkspaceArg,
) -> Result<(), McpError> {
    let task_ref = pb::TaskRef {
        line_number: line,
        task_id: String::new(),
    };
    let mutation = pb::Mutation {
        kind: Some(pb::mutation::Kind::Edit(pb::Edit {
            task: Some(task_ref),
            new_line: text,
        })),
    };
    apply_one(ctx, &file, mutation, workspace).await?;
    Ok(())
}
