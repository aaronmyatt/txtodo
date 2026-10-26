//! The `list` argument of every tool (task two-kinds-of-workspace, ADR 0043): a named list in place
//! of `workspace`, resolved by `call_tool` before any tool runs. In-process over `tokio::io::duplex`
//! like `smoke.rs`, under its lock for the same `tracing` reason.

use std::sync::Arc;

use rmcp::model::CallToolRequestParams;
use rmcp::{RoleClient, ServiceExt, service::RunningService};
use serde_json::{Value, json};
use txtodo_mcp::backend::WorkspaceInfo;
use txtodo_mcp::schema::McpServer;

use crate::fake_backend::FakeBackend;
use crate::smoke::SERIAL;

type Client = RunningService<RoleClient, ()>;

fn workspace(id: &str, name: &str, root: &str, is_list: bool) -> WorkspaceInfo {
    WorkspaceInfo {
        id: id.to_owned(),
        root: root.to_owned(),
        added_at_ms: 0,
        root_exists: true,
        has_state: true,
        name: name.to_owned(),
        is_list,
    }
}

/// A server over a fake daemon that holds a repo and two lists, `work` shown as `projects`.
#[allow(clippy::expect_used)]
async fn connect() -> (Arc<FakeBackend>, Client, tokio::task::JoinHandle<()>) {
    let backend = Arc::new(FakeBackend::default());
    *backend
        .workspaces
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(vec![
        workspace("01REPO", "txtodo", "/home/a/txtodo", false),
        workspace("01WORK", "projects", "/d/lists/work", true),
        workspace("01HOME", "home", "/d/lists/home", true),
    ]);
    let server = McpServer::new(backend.clone());
    let (server_io, client_io) = tokio::io::duplex(8192);
    let task = tokio::spawn(async move {
        let running = server.serve(server_io).await.expect("server initializes");
        running.waiting().await.expect("server task joins");
    });
    let client = ().serve(client_io).await.expect("client initializes");
    (backend, client, task)
}

/// `todo_list` with `args`; the error text when the server refuses.
#[allow(clippy::expect_used)]
async fn list(client: &Client, args: Value) -> Result<(), String> {
    let args = args.as_object().cloned().expect("object literal");
    let call = CallToolRequestParams::new("todo_list").with_arguments(args);
    client
        .peer()
        .call_tool(call)
        .await
        .map(|_| ())
        .map_err(|e| e.to_string())
}

fn sent_to(backend: &FakeBackend) -> Option<String> {
    let seen =
        (backend.last_list_workspace.lock()).unwrap_or_else(std::sync::PoisonError::into_inner);
    seen.clone()
}

#[tokio::test]
#[allow(clippy::expect_used)]
async fn a_list_name_becomes_the_workspace_the_tool_runs_on() {
    let _serial = SERIAL.lock().await;
    let (backend, client, task) = connect().await;
    for (name, id) in [
        ("projects", "01WORK"),
        ("Work", "01WORK"),
        ("home", "01HOME"),
    ] {
        list(&client, json!({ "list": name })).await.unwrap();
        assert_eq!(sent_to(&backend).as_deref(), Some(id), "{name}");
    }
    // An empty or null list is no list: the call names no workspace.
    for none in [json!({"list": ""}), json!({"list": null}), json!({})] {
        list(&client, none).await.unwrap();
        assert_eq!(sent_to(&backend), None);
    }
    client.cancel().await.expect("client cancels cleanly");
    task.await.expect("server task joins");
}

#[tokio::test]
#[allow(clippy::expect_used)]
async fn a_name_that_is_no_list_is_refused_and_names_the_lists() {
    let _serial = SERIAL.lock().await;
    let (backend, client, task) = connect().await;
    // A repo is not a list; neither is a name nobody has.
    for bad in ["txtodo", "nope"] {
        let why = list(&client, json!({ "list": bad })).await.unwrap_err();
        assert!(why.contains("no list named"), "{bad}: {why}");
        assert!(why.contains("projects, home"), "{bad}: {why}");
    }
    let both = json!({ "list": "home", "workspace": "/somewhere" });
    let why = list(&client, both).await.unwrap_err();
    assert!(why.contains("not both"), "{why}");
    assert_eq!(sent_to(&backend), None, "nothing ran");
    client.cancel().await.expect("client cancels cleanly");
    task.await.expect("server task joins");
}

#[tokio::test]
#[allow(clippy::expect_used)]
async fn every_tool_that_takes_a_workspace_advertises_the_list_argument() {
    let _serial = SERIAL.lock().await;
    let (_backend, client, task) = connect().await;
    let tools = client.peer().list_tools(None).await.expect("tools/list");
    let mut with_workspace = 0;
    for tool in &tools.tools {
        let properties = &tool.input_schema["properties"];
        if properties.get("workspace").is_some() {
            with_workspace += 1;
            assert!(properties["list"].is_object(), "{} lacks list", tool.name);
        }
    }
    assert!(
        with_workspace >= 15,
        "the tools that take a workspace: {with_workspace}"
    );
    client.cancel().await.expect("client cancels cleanly");
    task.await.expect("server task joins");
}
