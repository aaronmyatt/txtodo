//! `todotxt://` resources (design §6.4). The static `todo.txt` resource is listed;
//! parameterised ones (`task/{id}`, `project/{name}`, `context/{name}`, `history`) are resource
//! *templates* — `list_resources` never enumerates every task/project/context, only
//! `read_resource` resolves one. No subscription support here: `notifications/resources/updated`
//! push is [mcp-resource-subs](../../tasks/mcp-resource-subs), a separate task.
//!
//! Every shape (including the plain file path fallback) accepts an optional `?workspace=` query
//! param, sniffed the same way tool args are (mcp-multi-workspace-gateway notes.md) — no percent-
//! decoding, a documented limitation: a workspace value containing `&`/`?` cannot be expressed
//! this way today. `todotxt://workspaces` (`todo_list_workspaces`) is the one exception: it lists
//! the device-global registry, not any one open workspace, so no selector applies to it.

use rmcp::ErrorData;
use rmcp::model::{
    ListResourceTemplatesResult, ListResourcesResult, ReadResourceResult, Resource,
    ResourceContents, ResourceTemplate,
};

use crate::backend::{GetTarget, ListArgs, McpBackend, TaskRow, WorkspaceArg};
use crate::error::McpError;
use crate::parse::Token;

const SCHEME: &str = "todotxt";

/// `list_resources`: the always-present root file, plus `todo_list_workspaces`. Every synced ref
/// path also gets a resource entry, so a workspace with sub-lists (`q4-roadmap/todo.txt`) is fully
/// discoverable — but only when exactly one workspace is open: `rmcp::ServerHandler::list_resources`
/// takes no per-call argument, so there is nowhere for a selector to live here (unlike
/// `read_resource`'s URI). When 0 or 2+ are open, `list_files` reports the daemon's own ambiguity
/// error, which this function swallows rather than failing the whole `resources/list` call — a
/// real, documented limitation of discovery, not of reads (this crate's As-built notes).
pub async fn list(backend: &dyn McpBackend) -> Result<ListResourcesResult, ErrorData> {
    let mut resources: Vec<Resource> = backend
        .list_files(None)
        .await
        .map(|files| {
            files
                .into_iter()
                .filter(|f| f.kind == "todo")
                .map(|f| {
                    Resource::new(format!("{SCHEME}://{}", f.path), f.path)
                        .with_mime_type("text/plain")
                })
                .collect()
        })
        .unwrap_or_default();
    resources.push(
        Resource::new(format!("{SCHEME}://workspaces"), "todo_list_workspaces")
            .with_mime_type("application/json"),
    );
    Ok(ListResourcesResult::with_all_items(resources))
}

/// `list_resource_templates`: the parameterised shapes `read_resource` accepts.
pub fn list_templates() -> ListResourceTemplatesResult {
    let templates = vec![
        ResourceTemplate::new(format!("{SCHEME}://task/{{id}}{{?workspace}}"), "task"),
        ResourceTemplate::new(
            format!("{SCHEME}://project/{{name}}{{?workspace}}"),
            "project",
        ),
        ResourceTemplate::new(
            format!("{SCHEME}://context/{{name}}{{?workspace}}"),
            "context",
        ),
        ResourceTemplate::new(format!("{SCHEME}://history{{?since,workspace}}"), "history"),
    ];
    ListResourceTemplatesResult::with_all_items(templates)
}

/// `read_resource`: dispatches on the `todotxt://` URI shape.
pub async fn read(backend: &dyn McpBackend, uri: &str) -> Result<ReadResourceResult, ErrorData> {
    let tail = uri
        .strip_prefix(&format!("{SCHEME}://"))
        .ok_or_else(|| McpError::invalid_params(format!("not a {SCHEME}:// uri: {uri}")))?;
    let (path, workspace) = split_workspace_query(tail);
    let text = match path.split_once('/') {
        Some(("task", id)) => task_json(backend, id, workspace).await?,
        Some(("project", name)) => filtered_json(backend, Token::Project(name), workspace).await?,
        Some(("context", name)) => filtered_json(backend, Token::Context(name), workspace).await?,
        _ if path == "workspaces" => workspaces_json(backend).await?,
        _ if path == "history" || path.starts_with("history?") => {
            history_json(backend, &path, workspace).await?
        }
        _ => backend.get_file(Some(path.to_owned()), workspace).await?,
    };
    Ok(ReadResourceResult::new(vec![ResourceContents::text(
        text, uri,
    )]))
}

/// Splits the `?workspace=...` query param (if present) off the rest of `tail`, returning the
/// remaining path/query (other params, e.g. `since=`, kept as-is) and the extracted value. No
/// percent-decoding (see module doc).
fn split_workspace_query(tail: &str) -> (String, WorkspaceArg) {
    let Some((base, query)) = tail.split_once('?') else {
        return (tail.to_owned(), None);
    };
    let mut workspace = None;
    let mut rest = Vec::new();
    for kv in query.split('&').filter(|s| !s.is_empty()) {
        match kv.strip_prefix("workspace=") {
            Some(v) => workspace = Some(v.to_owned()),
            None => rest.push(kv),
        }
    }
    let rebuilt = if rest.is_empty() {
        base.to_owned()
    } else {
        format!("{base}?{}", rest.join("&"))
    };
    (rebuilt, workspace)
}

async fn task_json(
    backend: &dyn McpBackend,
    id: &str,
    workspace: WorkspaceArg,
) -> Result<String, McpError> {
    let row = backend
        .get(GetTarget {
            id: Some(id.to_owned()),
            workspace,
            ..GetTarget::default()
        })
        .await?;
    Ok(serde_json::to_string(&row).unwrap_or_default())
}

async fn filtered_json(
    backend: &dyn McpBackend,
    token: Token<'_>,
    workspace: WorkspaceArg,
) -> Result<String, McpError> {
    let rows = rows_with_token(backend, token, workspace).await?;
    Ok(serde_json::to_string(&rows).unwrap_or_default())
}

/// The rows of `todo.txt` carrying exactly `token`: `todo_list`'s substring query narrows them,
/// [`Token::is_on`] keeps the exact ones (`+work` is not `+workshop`). Shared with `triage_inbox`.
pub(crate) async fn rows_with_token(
    backend: &dyn McpBackend,
    token: Token<'_>,
    workspace: WorkspaceArg,
) -> Result<Vec<TaskRow>, McpError> {
    let mut rows = backend
        .list(ListArgs {
            query: Some(token.query()),
            done: None,
            file: None,
            limit: None,
            workspace,
        })
        .await?;
    rows.retain(|row| token.is_on(row));
    Ok(rows)
}

/// `todotxt://workspaces` (`todo_list_workspaces`): the device-global registry, unscoped.
async fn workspaces_json(backend: &dyn McpBackend) -> Result<String, McpError> {
    let workspaces = backend.list_workspaces().await?;
    Ok(serde_json::to_string(&workspaces).unwrap_or_default())
}

/// `todotxt://history` or `todotxt://history?since=<wall_ms>`. Only `since` (a raw millisecond
/// timestamp) is accepted; the counter half of an HLC has no natural place in a query string.
async fn history_json(
    backend: &dyn McpBackend,
    path: &str,
    workspace: WorkspaceArg,
) -> Result<String, McpError> {
    let since = path
        .split_once("since=")
        .and_then(|(_, v)| v.parse::<u64>().ok())
        .map(|wall_ms| crate::backend::Hlc {
            wall_ms,
            counter: 0,
        });
    let ops = backend.history(since, None, None, workspace).await?;
    Ok(serde_json::to_string(&ops).unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_templates_covers_every_parameterised_shape() {
        let templates = list_templates().resource_templates;
        let names: Vec<&str> = templates.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, ["task", "project", "context", "history"]);
    }

    #[test]
    fn split_workspace_query_extracts_workspace_and_keeps_other_params() {
        assert_eq!(
            split_workspace_query("todo.txt"),
            ("todo.txt".to_owned(), None)
        );
        assert_eq!(
            split_workspace_query("todo.txt?workspace=01ABCDEFGHJKMNPQRSTVWXYZ01"),
            (
                "todo.txt".to_owned(),
                Some("01ABCDEFGHJKMNPQRSTVWXYZ01".to_owned())
            )
        );
        assert_eq!(
            split_workspace_query("history?since=5&workspace=/some/dir"),
            ("history?since=5".to_owned(), Some("/some/dir".to_owned()))
        );
    }
}
