//! `todotxt://` resources (design §6.4). The static `todo.txt` resource is listed;
//! parameterised ones (`task/{id}`, `project/{name}`, `context/{name}`, `history`) are resource
//! *templates* — `list_resources` never enumerates every task/project/context, only
//! `read_resource` resolves one. No subscription support here: `notifications/resources/updated`
//! push is [mcp-resource-subs](../../tasks/mcp-resource-subs), a separate task.

use rmcp::ErrorData;
use rmcp::model::{
    ListResourceTemplatesResult, ListResourcesResult, ReadResourceResult, Resource,
    ResourceContents, ResourceTemplate,
};

use crate::backend::{GetTarget, ListArgs, McpBackend};
use crate::error::McpError;

const SCHEME: &str = "todotxt";

/// `list_resources`: the always-present root file. Every synced ref path also gets a resource
/// entry, so a workspace with sub-lists (`q4-roadmap/todo.txt`) is fully discoverable.
pub async fn list(backend: &dyn McpBackend) -> Result<ListResourcesResult, ErrorData> {
    let files = backend.list_files().await?;
    let resources = files
        .into_iter()
        .filter(|f| f.kind == "todo")
        .map(|f| {
            Resource::new(format!("{SCHEME}://{}", f.path), f.path).with_mime_type("text/plain")
        })
        .collect();
    Ok(ListResourcesResult::with_all_items(resources))
}

/// `list_resource_templates`: the parameterised shapes `read_resource` accepts.
pub fn list_templates() -> ListResourceTemplatesResult {
    let templates = vec![
        ResourceTemplate::new(format!("{SCHEME}://task/{{id}}"), "task"),
        ResourceTemplate::new(format!("{SCHEME}://project/{{name}}"), "project"),
        ResourceTemplate::new(format!("{SCHEME}://context/{{name}}"), "context"),
        ResourceTemplate::new(format!("{SCHEME}://history{{?since}}"), "history"),
    ];
    ListResourceTemplatesResult::with_all_items(templates)
}

/// `read_resource`: dispatches on the `todotxt://` URI shape.
pub async fn read(backend: &dyn McpBackend, uri: &str) -> Result<ReadResourceResult, ErrorData> {
    let path = uri
        .strip_prefix(&format!("{SCHEME}://"))
        .ok_or_else(|| McpError::invalid_params(format!("not a {SCHEME}:// uri: {uri}")))?;
    let text = match path.split_once('/') {
        Some(("task", id)) => task_json(backend, id).await?,
        Some(("project", name)) => filtered_json(backend, &format!("+{name}")).await?,
        Some(("context", name)) => filtered_json(backend, &format!("@{name}")).await?,
        _ if path == "history" || path.starts_with("history?") => {
            history_json(backend, path).await?
        }
        _ => backend.get_file(path.to_owned()).await?,
    };
    Ok(ReadResourceResult::new(vec![ResourceContents::text(
        text, uri,
    )]))
}

async fn task_json(backend: &dyn McpBackend, id: &str) -> Result<String, McpError> {
    let row = backend
        .get(GetTarget {
            id: Some(id.to_owned()),
            ..GetTarget::default()
        })
        .await?;
    Ok(serde_json::to_string(&row).unwrap_or_default())
}

async fn filtered_json(backend: &dyn McpBackend, query: &str) -> Result<String, McpError> {
    let rows = backend
        .list(ListArgs {
            query: Some(query.to_owned()),
            file: None,
            limit: None,
        })
        .await?;
    Ok(serde_json::to_string(&rows).unwrap_or_default())
}

/// `todotxt://history` or `todotxt://history?since=<wall_ms>`. Only `since` (a raw millisecond
/// timestamp) is accepted; the counter half of an HLC has no natural place in a query string.
async fn history_json(backend: &dyn McpBackend, path: &str) -> Result<String, McpError> {
    let since = path
        .split_once("since=")
        .and_then(|(_, v)| v.parse::<u64>().ok())
        .map(|wall_ms| crate::backend::Hlc {
            wall_ms,
            counter: 0,
        });
    let ops = backend.history(since, None, None).await?;
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
}
