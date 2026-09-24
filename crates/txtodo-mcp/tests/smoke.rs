//! In-process MCP smoke test (mcp-server-tools notes.md acceptance: "the registered set is
//! exhaustive"). Runs `McpServer` over `tokio::io::duplex` — no stdio process, no real daemon —
//! against a `FakeBackend` recording calls. The stock-SDK-client test over both real transports
//! against a real `txtodod` is `tests/sdk_client.rs` ([mcp-sdk-smoke](../../../tasks/mcp-sdk-smoke)).

use std::sync::Arc;

use rmcp::model::{CallToolRequestParams, GetPromptRequestParams, ReadResourceRequestParams};
use rmcp::{ClientHandler, ServiceExt};
use serde_json::json;

#[path = "smoke/fake_backend.rs"]
mod fake_backend;

use fake_backend::FakeBackend;
use txtodo_mcp::schema::McpServer;

/// task `mcp-smoke-span-flake`: every test below takes this lock for its duration, serializing
/// them against `tracing`'s process-global callsite-interest cache (and other global state
/// `tracing`/`rmcp` don't document as safe under concurrent ad-hoc subscribers) — see this task's
/// `notes.md` "As built" section for the full root-cause writeup. Dependency-free equivalent of
/// `serial_test`, scoped to this file. `tokio::sync::Mutex`, not `std`'s: every holder keeps it
/// locked across real `.await` points, which `clippy::await_holding_lock` refuses for a std guard.
static SERIAL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

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
    // task `mcp-hygiene-parity` / `mcp-conflicts-parity`
    "todo_lint",
    "todo_conflicts_list",
    "todo_conflicts_resolve",
];

/// The `txtodo` CLI commands MCP deliberately never mirrors (root todo
/// id:01M2T868JDPHR2RE0B1WSYZTRH). Each changes who or what this device trusts, or rewrites every
/// file of a workspace, and an agent session must not be able to do it: `workspace add|remove` (the
/// device-global registry), `pair` (cross-device pairing), `device remove` (group-key rotation and
/// device trust), `identity migrate` (rewrites every file in a workspace), and `fmt` (the one
/// command that rewrites lines it was not asked about, held until agent attribution covers a
/// whole-file rewrite). The allow-list above is closed, so a new tool cannot appear by accident;
/// this list is why these names must never be on it. See `crates/txtodo-mcp/CLAUDE.md`.
const NEVER_MIRRORED: &[&str] = &[
    "workspace",
    "pair",
    "device",
    "identity",
    "migrate",
    "fmt",
    "bundle",
    "daemon",
    "relay",
    "skill",
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

/// Every test in this file shares one `McpServer`/`tracing` process; see [`SERIAL`]'s own doc for
/// why they all take this lock rather than running with the usual per-test parallelism.
#[tokio::test]
async fn tool_list_is_the_exact_set() {
    let _serial = SERIAL.lock().await;
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
    // Task complete-to-bottom: completing moves the line, so an agent must be told that line
    // numbers it read before are stale and the id is what stays valid.
    let complete = tools.tools.iter().find(|t| t.name == "todo_complete");
    let description = complete
        .and_then(|t| t.description.as_deref())
        .unwrap_or_default();
    assert!(
        description.contains("bottom") && description.contains("id stays valid"),
        "todo_complete says where the line went: {description}"
    );
    client.cancel().await.expect("client cancels cleanly");
    server_task.await.expect("server task joins");
}

/// No registered tool is a mirror of a trust-boundary CLI command: none of its name's words is one
/// of [`NEVER_MIRRORED`]. Complements the exact-set test above, which already fails on any new tool.
#[tokio::test]
async fn no_tool_mirrors_a_trust_boundary_cli_command() {
    let _serial = SERIAL.lock().await;
    let (_backend, client, server_task) = connect().await;
    let tools = client
        .peer()
        .list_tools(None)
        .await
        .expect("tools/list succeeds");
    for tool in &tools.tools {
        for word in tool.name.split('_') {
            assert!(
                !NEVER_MIRRORED.contains(&word),
                "{} mirrors a CLI command MCP must never expose ({word})",
                tool.name
            );
        }
    }
    client.cancel().await.expect("client cancels cleanly");
    server_task.await.expect("server task joins");
}

#[tokio::test]
async fn todo_list_call_round_trips_to_the_backend() {
    let _serial = SERIAL.lock().await;
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
    let _serial = SERIAL.lock().await;
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

/// `(tool, arguments, the one backend method it must reach)`, every registered tool at least once;
/// `todo_raw` twice, since its arguments pick a read or a write.
fn tool_routes() -> Vec<(&'static str, serde_json::Value, &'static str)> {
    let id = "01M2T868JD32M84JQQ2ABASXW4";
    vec![
        ("todo_list", json!({"query": "milk", "limit": 3}), "list"),
        ("todo_search", json!({"text": "milk"}), "search"),
        ("todo_get", json!({"id": id}), "get"),
        ("todo_add", json!({"text": "buy milk"}), "add"),
        ("todo_complete", json!({"id": id}), "complete"),
        ("todo_uncomplete", json!({"id": id}), "complete"),
        (
            "todo_edit",
            json!({"id": id, "patch": {"append": "+p"}}),
            "edit",
        ),
        (
            "todo_move",
            json!({"id": id, "before": "01M2T868JD32M84JQQ2ABASXW5"}),
            "move_task",
        ),
        ("todo_delete", json!({"id": id, "confirm": true}), "delete"),
        ("todo_archive", json!({}), "archive"),
        ("todo_batch", json!({"ops": [], "dry_run": true}), "batch"),
        ("todo_history", json!({"id": id}), "history"),
        (
            "todo_raw",
            json!({"file": "todo.txt", "lines": [1]}),
            "raw_read",
        ),
        (
            "todo_raw",
            json!({"file": "todo.txt", "line": 1, "text": "x"}),
            "raw_write",
        ),
        ("todo_notes_get", json!({"id": id}), "notes_get"),
        (
            "todo_notes_set",
            json!({"id": id, "text": "# n"}),
            "notes_set",
        ),
        ("todo_lint", json!({}), "lint"),
        ("todo_conflicts_list", json!({}), "conflicts_list"),
        (
            "todo_conflicts_resolve",
            json!({"id": id, "side": "mine"}),
            "conflicts_resolve",
        ),
    ]
}

/// Every tool, called through the real JSON-RPC layer, reaches the one backend method it is meant
/// to: the argument schema parses, and nothing is routed to a neighbour (`todo_uncomplete` to
/// `complete`, `todo_raw` to a read or a write by its arguments). tasks/coverage-ratchet-climb:
/// `tools_write.rs` had no test reaching it at all.
#[tokio::test]
async fn every_tool_routes_to_its_own_backend_method() {
    let _serial = SERIAL.lock().await;
    let (backend, client, server_task) = connect().await;
    let routes = tool_routes();
    for (tool, args, expected) in routes {
        backend
            .calls
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clear();
        let args = args.as_object().cloned().expect("object literal");
        let result = client
            .peer()
            .call_tool(CallToolRequestParams::new(tool).with_arguments(args))
            .await
            .unwrap_or_else(|e| panic!("{tool}: {e}"));
        assert_ne!(result.is_error, Some(true), "{tool} reported an error");
        let calls = backend
            .calls
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        assert_eq!(calls, [expected], "{tool} reached the wrong backend method");
    }
    client.cancel().await.expect("client cancels cleanly");
    server_task.await.expect("server task joins");
}

/// tasks/logging-mcp-call-span: proves the `mcp.call{tool,principal}` span (plan §5,
/// `txtodo-implementation-plan.md:447`) actually lands on a JSON log line with the right name and
/// fields — not just "it compiled". `txtodo_telemetry::testing::LogSink` is the shared capture
/// seam every crate's own tests use; this test builds its own `with_span_events(FmtSpan::CLOSE)`
/// layer on top of it (see this crate's `Cargo.toml` for why `capturing_dispatch` alone isn't
/// enough — a span that closes with no event inside it never otherwise reaches the writer).
///
/// task `mcp-smoke-span-flake`: this test used to flake under concurrent test execution (`tracing`
/// callsite-interest is process-global, not per-`Dispatch` — see [`SERIAL`]'s own doc and this
/// task's `notes.md` "As built" section for the full root-cause writeup); fixed by taking
/// [`SERIAL`] for the duration, confirmed by 30 consecutive full-suite runs with zero failures.
#[tokio::test]
async fn mcp_call_span_names_tool_and_records_principal() {
    use tracing_subscriber::fmt::format::FmtSpan;
    use tracing_subscriber::layer::SubscriberExt;

    let _serial = SERIAL.lock().await;
    let sink = txtodo_telemetry::testing::LogSink::new();
    let subscriber = tracing_subscriber::registry().with(
        tracing_subscriber::fmt::layer()
            .json()
            .with_span_events(FmtSpan::CLOSE)
            .with_writer(sink.clone()),
    );
    let _guard = tracing::subscriber::set_default(subscriber);
    tracing::callsite::rebuild_interest_cache();

    let (_backend, client, server_task) = connect().await;
    let args = json!({}).as_object().cloned().expect("object literal");
    let _ = client
        .peer()
        .call_tool(CallToolRequestParams::new("todo_list").with_arguments(args))
        .await
        .expect("tools/call succeeds");
    client.cancel().await.expect("client cancels cleanly");
    server_task.await.expect("server task joins");

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    let (text, line) = loop {
        let text = sink.captured_text();
        if let Some(l) = text.lines().find(|l| l.contains("\"mcp.call\"")) {
            break (text.clone(), l.to_owned());
        }
        assert!(
            std::time::Instant::now() < deadline,
            "no mcp.call span line in captured output within 2s: {text}"
        );
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    };
    let value: serde_json::Value = serde_json::from_str(&line).expect("captured line is JSON");
    let span = &value["span"];
    assert_eq!(span["name"], "mcp.call", "{line}");
    assert_eq!(span["tool"], "todo_list", "{line}");
    assert_eq!(span["principal"], "user", "{line}");
    // The bearer/secret side of a principal never appears — `FakeBackend::principal` returns
    // "user" (the unauthenticated default), so this also doubles as proof no token text leaks.
    assert!(!text.contains("token_id"), "{text}");
}
