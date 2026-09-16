//! Read-side [`crate::backend::McpBackend`] logic, plus [`locate_by_id`] (the cross-file task-id
//! lookup the write side also needs). Split out of `grpc_backend.rs` for the file budget.

use tonic::transport::Channel;
use txtodo_proto::v1 as pb;
use txtodo_proto::v1::txtodo_client::TxtodoClient;

use crate::backend::Hlc;
use crate::backend::{FileMeta, GetTarget, ListArgs, OpSummary, RefPath, TaskId, TaskRow};
use crate::backend::{WorkspaceArg, WorkspaceInfo};
use crate::error::McpError;
use crate::grpc_convert::{file_meta, op_summary, workspace_info, workspace_selector};
use crate::parse;

/// `todo.txt` at the workspace root; every `file`-taking tool defaults to it.
pub const DEFAULT_TODO: &str = "todo.txt";

fn status(s: tonic::Status) -> McpError {
    McpError::daemon(s.message().to_owned())
}

/// Whole-file bytes as UTF-8 text (lossy — a byte-exact round trip is the CLI's job, not this read
/// path's).
pub async fn get_file_text(
    mut client: TxtodoClient<Channel>,
    path: &str,
    workspace: WorkspaceArg,
) -> Result<String, McpError> {
    let req = pb::GetFileRequest {
        path: path.to_owned(),
        workspace: workspace_selector(workspace),
    };
    let rep = client.get_file(req).await.map_err(status)?;
    Ok(String::from_utf8_lossy(&rep.into_inner().bytes).into_owned())
}

/// Every synced document (`ListFiles`).
pub async fn list_files(
    mut client: TxtodoClient<Channel>,
    workspace: WorkspaceArg,
) -> Result<Vec<FileMeta>, McpError> {
    let rep = client
        .list_files(pb::ListFilesRequest {
            workspace: workspace_selector(workspace),
        })
        .await
        .map_err(status)?;
    Ok(rep
        .into_inner()
        .files
        .iter()
        .filter_map(file_meta)
        .collect())
}

/// Every registered workspace (`WorkspaceList`; `todo_list_workspaces` resource). Device-global —
/// no selector applies.
pub async fn list_workspaces(
    mut client: TxtodoClient<Channel>,
) -> Result<Vec<WorkspaceInfo>, McpError> {
    let rep = client
        .workspace_list(pb::WorkspaceListRequest {})
        .await
        .map_err(status)?;
    Ok(rep
        .into_inner()
        .workspaces
        .into_iter()
        .map(workspace_info)
        .collect())
}

/// `todo_list`.
pub async fn list(client: TxtodoClient<Channel>, args: ListArgs) -> Result<Vec<TaskRow>, McpError> {
    let path = args.file.clone().unwrap_or_else(|| DEFAULT_TODO.to_owned());
    let text = get_file_text(client, &path, args.workspace.clone()).await?;
    let mut rows: Vec<TaskRow> = parse::lines(&text)
        .into_iter()
        .map(|(n, l)| parse::parse_row(n, l))
        .filter(|r| !r.raw.trim().is_empty())
        .collect();
    if let Some(q) = &args.query {
        rows.retain(|r| parse::matches_minimal_query(r, q));
    }
    if let Some(limit) = args.limit.filter(|&l| l > 0) {
        rows.truncate(limit as usize);
    }
    Ok(rows)
}

/// `todo_search`: case-insensitive substring over `raw`. Design §8 calls for a tantivy full-text
/// index "owned by the daemon backend" — no such index exists yet (see the crate's As-built
/// notes), so this is the honest placeholder until that infrastructure lands.
pub async fn search(
    client: TxtodoClient<Channel>,
    text: String,
    file: Option<RefPath>,
    workspace: WorkspaceArg,
) -> Result<Vec<TaskRow>, McpError> {
    let rows = list(
        client,
        ListArgs {
            query: None,
            file,
            limit: None,
            workspace,
        },
    )
    .await?;
    let needle = text.to_lowercase();
    Ok(rows
        .into_iter()
        .filter(|r| r.raw.to_lowercase().contains(&needle))
        .collect())
}

/// `todo_get`.
pub async fn get(client: TxtodoClient<Channel>, target: GetTarget) -> Result<TaskRow, McpError> {
    if let Some(id) = &target.id {
        let (_, _, row) = locate_by_id(client, id, target.workspace).await?;
        return Ok(row);
    }
    let Some(line) = target.line else {
        return Err(McpError::invalid_params("todo_get needs id or line"));
    };
    let path = target.file.unwrap_or_else(|| DEFAULT_TODO.to_owned());
    let text = get_file_text(client, &path, target.workspace).await?;
    row_at_line(&text, line)
}

fn row_at_line(text: &str, line: u32) -> Result<TaskRow, McpError> {
    parse::lines(text)
        .into_iter()
        .find(|(n, _)| *n == line)
        .map(|(n, raw)| parse::parse_row(n, raw))
        .ok_or_else(|| McpError::not_found(format!("no line {line}")).with_line(line))
}

/// Finds the task across every `todo` document in `workspace`. An `id` lookup has no path to start
/// from — unlike `GetNotes`/`EditNotes`'s wire `TaskRef`, which the daemon itself resolves by id
/// (`notes_lookup.rs`), the general `Apply`/`GetFile` RPCs need a real `path` + `line_number`, so
/// the client scans (bounded: workspace document counts are already capped at 10 000).
pub async fn locate_by_id(
    client: TxtodoClient<Channel>,
    id: &TaskId,
    workspace: WorkspaceArg,
) -> Result<(RefPath, u32, TaskRow), McpError> {
    let files = list_files(client.clone(), workspace.clone()).await?;
    for f in files.iter().filter(|f| f.kind == "todo") {
        let text = get_file_text(client.clone(), &f.path, workspace.clone()).await?;
        if let Some((line, raw)) = parse::find_by_id(&text, id) {
            return Ok((f.path.clone(), line, parse::parse_row(line, raw)));
        }
    }
    Err(McpError::not_found(format!("no task id:{id}")))
}

/// `todo_history`.
pub async fn history(
    mut client: TxtodoClient<Channel>,
    since: Option<Hlc>,
    id: Option<TaskId>,
    file: Option<RefPath>,
    workspace: WorkspaceArg,
) -> Result<Vec<OpSummary>, McpError> {
    let req = pb::HistoryRequest {
        path: file.unwrap_or_default(),
        task_id: id.unwrap_or_default(),
        limit: 0,
        before_seq: 0,
        workspace: workspace_selector(workspace),
    };
    let rep = client.history(req).await.map_err(status)?;
    let mut ops: Vec<OpSummary> = rep.into_inner().ops.into_iter().map(op_summary).collect();
    if let Some(since) = since {
        ops.retain(|o| (o.hlc.wall_ms, o.hlc.counter) >= (since.wall_ms, since.counter));
    }
    Ok(ops)
}

/// `todo_raw`, read mode.
pub async fn raw_read(
    client: TxtodoClient<Channel>,
    file: RefPath,
    lines: Vec<u32>,
    workspace: WorkspaceArg,
) -> Result<Vec<String>, McpError> {
    let text = get_file_text(client, &file, workspace).await?;
    let all = parse::lines(&text);
    lines
        .into_iter()
        .map(|n| {
            all.iter()
                .find(|(ln, _)| *ln == n)
                .map(|(_, raw)| (*raw).to_owned())
                .ok_or_else(|| McpError::not_found(format!("no line {n}")).with_line(n))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn row_at_line_finds_and_reports_a_missing_line() {
        let text = "a\nb\nc\n";
        assert_eq!(row_at_line(text, 2).map(|r| r.raw), Ok("b".to_owned()));
        let err = row_at_line(text, 9).unwrap_err();
        assert_eq!(err.line, Some(9));
        assert_eq!(err.code, "not_found");
    }
}
