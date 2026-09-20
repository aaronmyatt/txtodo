//! The HTTP transport's `Host` and `Origin` guard (task mcp-local-only). Loopback alone is not
//! private: a browser tab can post to `127.0.0.1`, and a hostile page can rebind its own name to
//! it. Driven through the router itself with `tower::ServiceExt::oneshot`, so no port is bound.
//! https://docs.rs/tower/latest/tower/trait.ServiceExt.html#method.oneshot

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use tokio_util::sync::CancellationToken;
use tower::ServiceExt;

#[path = "smoke/fake_backend.rs"]
mod fake_backend;

use fake_backend::FakeBackend;
use txtodo_mcp::schema::McpServer;
use txtodo_mcp::transport::{MCP_PATH, MCP_PORT, http_router};

/// An MCP `initialize` POST with the given `Host` and optional `Origin`, answered by the router.
async fn status_for(host: &str, origin: Option<&str>) -> StatusCode {
    let server = McpServer::new(Arc::new(FakeBackend::default()));
    let router = http_router(server, MCP_PORT, CancellationToken::new());
    let body = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"t","version":"0"}}}"#;
    let mut request = Request::post(MCP_PATH)
        .header("host", host)
        .header("content-type", "application/json")
        .header("accept", "application/json, text/event-stream");
    if let Some(origin) = origin {
        request = request.header("origin", origin);
    }
    let request = request
        .body(Body::from(body))
        .unwrap_or_else(|e| panic!("request: {e}"));
    router
        .oneshot(request)
        .await
        .unwrap_or_else(|e| panic!("router: {e}"))
        .status()
}

#[tokio::test]
async fn a_foreign_origin_gets_403() {
    let status = status_for("127.0.0.1:8636", Some("https://evil.example")).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    // Loopback on another port is another origin too: (scheme, host, port) must all match.
    let other_port = status_for("127.0.0.1:8636", Some("http://127.0.0.1:9999")).await;
    assert_eq!(other_port, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn a_rebound_host_gets_403() {
    // The DNS-rebinding case: the socket is loopback, the name in `Host` is the attacker's.
    let status = status_for("evil.example:8636", None).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn a_local_client_with_no_origin_or_our_own_origin_is_served() {
    let plain = status_for("127.0.0.1:8636", None).await;
    assert_ne!(
        plain,
        StatusCode::FORBIDDEN,
        "every non-browser MCP client sends no Origin"
    );
    assert!(plain.is_success(), "initialize answers: {plain}");
    let own = status_for("localhost:8636", Some("http://localhost:8636")).await;
    assert_ne!(own, StatusCode::FORBIDDEN);
}
