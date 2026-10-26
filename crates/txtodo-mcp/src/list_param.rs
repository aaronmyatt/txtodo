//! The `list` argument of every tool (task `two-kinds-of-workspace`, ADR 0043): a named list, by
//! the name it shows or the name it was made under, in place of `workspace`.
//!
//! Resolved here, once, before a tool runs: `call_tool` asks the daemon for the workspaces, picks
//! the list by the rule every client shares (`txtodo_workspace_paths::pick_list`) and rewrites the
//! arguments to carry that list's `workspace` id, so no tool body knows lists exist. The property
//! is added to each tool's published schema the same way, so an agent reading the schema sees it.
//!
//! MCP uses lists and never makes them: a name that is no list is refused, naming the lists there
//! are, and `lists new|rename|rm|import` stay out of the tool set like `workspace add|remove`
//! (`tests/it/smoke.rs`, `CLAUDE.md`). An agent that wanted a new list would be adding a list that
//! syncs to every one of the user's devices.
//! Ref: <https://docs.rs/rmcp/latest/rmcp/model/struct.Tool.html>

use std::path::PathBuf;
use std::sync::Arc;

use rmcp::handler::server::tool::ToolCallContext;
use rmcp::model::{CallToolRequestParams, CallToolResponse, JsonObject, Tool};
use rmcp::service::RequestContext;
use rmcp::{ErrorData, RoleServer};
use serde_json::{Value, json};
use txtodo_workspace_paths::{ListRef, Pick, pick_list};

use crate::backend::{McpBackend, WorkspaceInfo};
use crate::fallback;
use crate::schema::McpServer;

const DESCRIPTION: &str = "A txtodo list to use instead of `workspace`: the name it shows, or \
    the name it was made under. A name that is no list is refused, and this tool cannot make one \
    (the user runs `txtodo lists new NAME`). Give a list or a workspace, not both.";

/// Adds the `list` property to `tool` when it has a `workspace` one.
pub(crate) fn add_to_schema(tool: &mut Tool) {
    let schema = Arc::make_mut(&mut tool.input_schema);
    let Some(Value::Object(properties)) = schema.get_mut("properties") else {
        return;
    };
    if properties.contains_key("workspace") {
        let property = json!({ "type": ["string", "null"], "description": DESCRIPTION });
        properties.insert("list".to_owned(), property);
    }
}

/// `call_tool`: scopes a `list` argument to its workspace, runs the tool, and ends the result with
/// the fallback note when the call named no workspace (`fallback.rs`).
pub(crate) async fn call(
    server: &McpServer,
    mut request: CallToolRequestParams,
    context: RequestContext<RoleServer>,
) -> Result<CallToolResponse, ErrorData> {
    scope(server.backend.as_ref(), &mut request.arguments).await?;
    let named = fallback::names_workspace(request.arguments.as_ref());
    let call = ToolCallContext::new(server, request, context);
    let response = server.tool_router.call(call).await?;
    Ok(fallback::add_note(
        response,
        server.fallback_note.as_deref(),
        named,
    ))
}

/// Replaces a `list` argument with the `workspace` it means. A missing, null or empty `list` is no
/// list; one beside a `workspace` is refused.
async fn scope(
    backend: &dyn McpBackend,
    arguments: &mut Option<JsonObject>,
) -> Result<(), ErrorData> {
    let Some(args) = arguments.as_mut() else {
        return Ok(());
    };
    let wanted = args
        .remove("list")
        .and_then(|v| v.as_str().map(|s| s.trim().to_owned()))
        .filter(|name| !name.is_empty());
    let Some(name) = wanted else {
        return Ok(());
    };
    if fallback::names_workspace(Some(&*args)) {
        let why = "give a list or a workspace, not both";
        return Err(ErrorData::invalid_params(why, None));
    }
    let all = backend.list_workspaces().await.map_err(ErrorData::from)?;
    let id = pick(&all, &name).map_err(|why| ErrorData::invalid_params(why, None))?;
    args.insert("workspace".to_owned(), Value::String(id));
    Ok(())
}

/// The id of the list `name` means among `all`. The error names the lists there are, so an agent
/// can retry with one.
fn pick(all: &[WorkspaceInfo], name: &str) -> Result<String, String> {
    let lists: Vec<&WorkspaceInfo> = all.iter().filter(|w| w.is_list).collect();
    let roots: Vec<PathBuf> = lists.iter().map(|w| PathBuf::from(&w.root)).collect();
    let refs: Vec<ListRef<'_>> = lists
        .iter()
        .zip(&roots)
        .map(|(w, root)| ListRef {
            shown: &w.name,
            root,
        })
        .collect();
    match pick_list(&refs, name) {
        Pick::Found(i) => Ok(lists[i].id.clone()),
        Pick::Ambiguous(_) => Err(format!(
            "more than one list shows {name:?}; name one by the name it was made under"
        )),
        Pick::NotFound => {
            let names: Vec<&str> = lists.iter().map(|w| w.name.as_str()).collect();
            Err(format!(
                "no list named {name:?}; the lists here are: {}",
                if names.is_empty() {
                    "(none)".to_owned()
                } else {
                    names.join(", ")
                }
            ))
        }
    }
}

#[cfg(test)]
#[path = "list_param_tests.rs"]
mod tests;
