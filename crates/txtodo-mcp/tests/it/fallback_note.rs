//! Task two-kinds-of-workspace: a call that named no workspace on a server that fell back to the
//! default says so in its result, as a second text block. A call that named one does not, and a
//! server with a workspace of its own adds nothing. In-process over `tokio::io::duplex`, like
//! `smoke.rs`, and under its lock for the same `tracing` reason.

use std::sync::Arc;

use rmcp::model::{CallToolRequestParams, CallToolResult};
use rmcp::{RoleClient, ServiceExt, service::RunningService};
use serde_json::json;

use crate::fake_backend::FakeBackend;
use crate::smoke::SERIAL;
use txtodo_mcp::schema::McpServer;

const NOTE: &str = "no workspace here; calls that name no workspace use the default workspace";

/// A client and the server task, the server built with `note` as its fallback note.
#[allow(clippy::expect_used)]
async fn connect(
    note: Option<&str>,
) -> (RunningService<RoleClient, ()>, tokio::task::JoinHandle<()>) {
    let server = McpServer::new(Arc::new(FakeBackend::default()))
        .with_fallback_note(note.map(str::to_owned));
    let (server_io, client_io) = tokio::io::duplex(8192);
    let task = tokio::spawn(async move {
        let running = server.serve(server_io).await.expect("server initializes");
        running.waiting().await.expect("server task joins");
    });
    let client = ().serve(client_io).await.expect("client initializes");
    (client, task)
}

/// `todo_list` with `args`.
#[allow(clippy::expect_used)]
async fn list(client: &RunningService<RoleClient, ()>, args: serde_json::Value) -> CallToolResult {
    let args = args.as_object().cloned().expect("object literal");
    client
        .peer()
        .call_tool(CallToolRequestParams::new("todo_list").with_arguments(args))
        .await
        .expect("tools/call succeeds")
}

#[tokio::test]
#[allow(clippy::expect_used)]
async fn a_call_that_fell_back_to_the_default_ends_with_the_note() {
    let _serial = SERIAL.lock().await;
    let (client, task) = connect(Some(NOTE)).await;
    let unnamed = list(&client, json!({})).await;
    let texts: Vec<_> = unnamed.content.iter().filter_map(|c| c.as_text()).collect();
    assert_eq!(texts.len(), 2, "rows, then the note: {texts:?}");
    assert_eq!(texts[1].text, NOTE, "the note is the last block");
    client.cancel().await.expect("client cancels cleanly");
    task.await.expect("server task joins");
}

#[tokio::test]
#[allow(clippy::expect_used)]
async fn a_call_naming_a_workspace_or_a_server_with_one_has_no_note() {
    let _serial = SERIAL.lock().await;
    let (client, task) = connect(Some(NOTE)).await;
    let named = list(&client, json!({"workspace": "/repo"})).await;
    assert_eq!(named.content.len(), 1, "a named workspace is no fallback");
    client.cancel().await.expect("client cancels cleanly");
    task.await.expect("server task joins");

    let (client, task) = connect(None).await;
    let plain = list(&client, json!({})).await;
    assert_eq!(plain.content.len(), 1, "no note, no extra block");
    client.cancel().await.expect("client cancels cleanly");
    task.await.expect("server task joins");
}
