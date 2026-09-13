//! In-process MCP smoke test (mcp-server-tools notes.md acceptance: "the registered set is
//! exhaustive"). Runs `McpServer` over `tokio::io::duplex` — no stdio process, no real daemon —
//! against a `FakeBackend` recording calls; a full SDK-reference-client smoke test over both real
//! transports is [mcp-smoke-test](../../../tasks/mcp-smoke-test), a separate later task.

use std::sync::{Arc, Mutex};

use rmcp::model::{CallToolRequestParams, GetPromptRequestParams, ReadResourceRequestParams};
use rmcp::{ClientHandler, ServiceExt};
use serde_json::json;

use txtodo_mcp::backend::{
    ApplyOutcome, FieldPatch, FileMeta, GetTarget, Hlc, ListArgs, McpBackend, MoveAnchor,
    OpSummary, RefPath, TaskId, TaskRow, TodoOp,
};
use txtodo_mcp::error::McpError;
use txtodo_mcp::schema::McpServer;

/// Records every call it receives and returns canned, deterministic data — no real daemon, no
/// filesystem. Exercises the wiring (schema → tools → backend → back out as JSON), not the gRPC
/// client (that's `grpc_backend.rs`'s job, covered by its own unit tests).
#[derive(Default)]
struct FakeBackend {
    calls: Mutex<Vec<&'static str>>,
}

impl FakeBackend {
    fn record(&self, name: &'static str) {
        self.calls
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(name);
    }
}

#[async_trait::async_trait]
impl McpBackend for FakeBackend {
    async fn list(&self, _args: ListArgs) -> Result<Vec<TaskRow>, McpError> {
        self.record("list");
        Ok(vec![TaskRow {
            id: Some("01J".to_owned()),
            line: 1,
            raw: "(A) 2026-09-11 Draft +work id:01J".to_owned(),
            priority: Some('A'),
            projects: vec!["work".to_owned()],
            ..TaskRow::default()
        }])
    }
    async fn search(&self, _t: String, _f: Option<RefPath>) -> Result<Vec<TaskRow>, McpError> {
        self.record("search");
        Ok(vec![])
    }
    async fn get(&self, _target: GetTarget) -> Result<TaskRow, McpError> {
        self.record("get");
        Ok(TaskRow::default())
    }
    async fn add(&self, _text: String, _file: Option<RefPath>) -> Result<TaskRow, McpError> {
        self.record("add");
        Ok(TaskRow::default())
    }
    async fn complete(&self, _id: TaskId, _done: bool) -> Result<TaskRow, McpError> {
        self.record("complete");
        Ok(TaskRow::default())
    }
    async fn edit(&self, _id: TaskId, _patch: FieldPatch) -> Result<TaskRow, McpError> {
        self.record("edit");
        Ok(TaskRow::default())
    }
    async fn move_task(&self, _id: TaskId, _anchor: MoveAnchor) -> Result<TaskRow, McpError> {
        self.record("move_task");
        Ok(TaskRow::default())
    }
    async fn delete(&self, _id: TaskId, _confirm: bool) -> Result<(), McpError> {
        self.record("delete");
        Ok(())
    }
    async fn archive(&self, _file: RefPath) -> Result<ApplyOutcome, McpError> {
        self.record("archive");
        Ok(ApplyOutcome::default())
    }
    async fn batch(&self, _ops: Vec<TodoOp>, _dry_run: bool) -> Result<ApplyOutcome, McpError> {
        self.record("batch");
        Ok(ApplyOutcome::default())
    }
    async fn history(
        &self,
        _since: Option<Hlc>,
        _id: Option<TaskId>,
        _file: Option<RefPath>,
    ) -> Result<Vec<OpSummary>, McpError> {
        self.record("history");
        Ok(vec![])
    }
    async fn raw_read(&self, _file: RefPath, _lines: Vec<u32>) -> Result<Vec<String>, McpError> {
        self.record("raw_read");
        Ok(vec![])
    }
    async fn raw_write(&self, _file: RefPath, _line: u32, _text: String) -> Result<(), McpError> {
        self.record("raw_write");
        Ok(())
    }
    async fn notes_get(&self, _id: TaskId) -> Result<String, McpError> {
        self.record("notes_get");
        Ok(String::new())
    }
    async fn notes_set(&self, _id: TaskId, _text: String) -> Result<(), McpError> {
        self.record("notes_set");
        Ok(())
    }
    async fn list_files(&self) -> Result<Vec<FileMeta>, McpError> {
        self.record("list_files");
        Ok(vec![FileMeta {
            path: "todo.txt".to_owned(),
            kind: "todo",
        }])
    }
    async fn get_file(&self, _file: RefPath) -> Result<String, McpError> {
        self.record("get_file");
        Ok("(A) 2026-09-11 Draft +work id:01J\n".to_owned())
    }
}

/// Every §6.3 tool plus `todo_notes_get`/`todo_notes_set` — the exact-match set mcp-server-tools
/// notes.md's acceptance criteria names.
const EXPECTED_TOOLS: &[&str] = &[
    "todo_list",
    "todo_search",
    "todo_get",
    "todo_add",
    "todo_complete",
    "todo_uncomplete",
    "todo_edit",
    "todo_move",
    "todo_delete",
    "todo_archive",
    "todo_batch",
    "todo_history",
    "todo_raw",
    "todo_notes_get",
    "todo_notes_set",
];

type ConnectedClient = rmcp::service::RunningService<rmcp::RoleClient, ()>;

/// Spins up `McpServer` and a bare `()` client over an in-memory duplex pipe (no stdio process,
/// no real daemon). Callers must `.cancel()` the client and `.await` the returned task when done.
/// `clippy::expect_used` is allowed explicitly: clippy's "allow in tests" heuristic only reaches
/// `#[test]`-attributed functions, not a plain helper an integration test crate's tests call.
#[allow(clippy::expect_used)]
async fn connect() -> (
    Arc<FakeBackend>,
    ConnectedClient,
    tokio::task::JoinHandle<()>,
) {
    let backend = Arc::new(FakeBackend::default());
    let server = McpServer::new(backend.clone());
    let (server_io, client_io) = tokio::io::duplex(8192);
    let server_task = tokio::spawn(async move {
        let running = server.serve(server_io).await.expect("server initializes");
        running.waiting().await.expect("server task joins");
    });
    let client = ().serve(client_io).await.expect("client initializes");
    (backend, client, server_task)
}

#[tokio::test]
async fn tool_list_is_the_exact_set() {
    let (_backend, client, server_task) = connect().await;
    let tools = client
        .peer()
        .list_tools(None)
        .await
        .expect("tools/list succeeds");
    let mut names: Vec<&str> = tools.tools.iter().map(|t| t.name.as_ref()).collect();
    names.sort_unstable();
    let mut expected: Vec<&str> = EXPECTED_TOOLS.to_vec();
    expected.sort_unstable();
    assert_eq!(names, expected, "the registered tool set is an exact match");
    client.cancel().await.expect("client cancels cleanly");
    server_task.await.expect("server task joins");
}

#[tokio::test]
async fn todo_list_call_round_trips_to_the_backend() {
    let (backend, client, server_task) = connect().await;
    let args = json!({}).as_object().cloned().expect("object literal");
    let result = client
        .peer()
        .call_tool(CallToolRequestParams::new("todo_list").with_arguments(args))
        .await
        .expect("tools/call succeeds");
    assert_ne!(
        result.is_error,
        Some(true),
        "todo_list did not report an error"
    );
    let saw_exactly_one_list_call = {
        let calls = backend
            .calls
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        calls.as_slice() == ["list"]
    };
    assert!(
        saw_exactly_one_list_call,
        "backend.list was called exactly once"
    );
    client.cancel().await.expect("client cancels cleanly");
    server_task.await.expect("server task joins");
}

#[tokio::test]
async fn resources_and_prompts_are_registered_and_readable() {
    let (_backend, client, server_task) = connect().await;
    let resources = client
        .peer()
        .list_resources(None)
        .await
        .expect("resources/list succeeds");
    assert!(
        resources
            .resources
            .iter()
            .any(|r| r.uri == "todotxt://todo.txt"),
        "todo.txt is always a listed resource"
    );
    let read = client
        .peer()
        .read_resource(ReadResourceRequestParams::new("todotxt://todo.txt"))
        .await
        .expect("resources/read succeeds");
    assert!(!read.contents.is_empty(), "todo.txt resource has content");
    let prompts = client
        .peer()
        .list_prompts(None)
        .await
        .expect("prompts/list succeeds");
    assert!(
        prompts.prompts.iter().any(|p| p.name == "plan_today"),
        "plan_today is registered"
    );
    let prompt = client
        .peer()
        .get_prompt(GetPromptRequestParams::new("plan_today"))
        .await
        .expect("prompts/get succeeds");
    assert!(!prompt.messages.is_empty(), "plan_today returns messages");
    client.cancel().await.expect("client cancels cleanly");
    server_task.await.expect("server task joins");
}

// `ReadResourceRequestParams`/`GetPromptRequestParams` need a `new` constructor; both exist on the
// real types, this just documents where they come from for anyone grepping this file later.
#[allow(dead_code)]
fn _construction_reference(u: &str) -> (ReadResourceRequestParams, GetPromptRequestParams) {
    (
        ReadResourceRequestParams::new(u),
        GetPromptRequestParams::new(u),
    )
}

// `()` implements `ClientHandler` (rmcp's blanket no-op client), which is all this smoke test
// needs — it never receives a server-initiated request.
#[allow(dead_code)]
fn _client_handler_reference<T: ClientHandler>() {}
