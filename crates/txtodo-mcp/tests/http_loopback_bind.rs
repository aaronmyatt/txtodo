//! The HTTP transport listens on loopback only (task security-m6-review, F5 / checklist row 4).
//! `serve_http` takes a port, not an address, so this is a regression guard: it starts the real
//! server, then dials it on 127.0.0.1 (served) and on this host's own non-loopback address
//! (refused). A bind to 0.0.0.0 would answer both.

use std::net::{IpAddr, SocketAddr, TcpListener, UdpSocket};
use std::sync::Arc;
use std::time::Duration;

use tokio::net::TcpStream;
use tokio_util::sync::CancellationToken;

#[path = "smoke/fake_backend.rs"]
mod fake_backend;

use fake_backend::FakeBackend;
use txtodo_mcp::schema::McpServer;
use txtodo_mcp::transport::{MCP_LOOPBACK, serve_http};

/// This host's source address on its default route, or `None` on a host with no network.
/// A UDP `connect` only picks a route and a source address; no packet is sent.
/// Ref: https://doc.rust-lang.org/std/net/struct.UdpSocket.html#method.connect
/// 192.0.2.1 is TEST-NET-1, reserved for documentation: https://www.rfc-editor.org/rfc/rfc5737
fn non_loopback_ip() -> Option<IpAddr> {
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("192.0.2.1:9").ok()?;
    let ip = socket.local_addr().ok()?.ip();
    (!ip.is_loopback() && !ip.is_unspecified()).then_some(ip)
}

/// A port nothing holds on any interface right now: bound on 0.0.0.0 with port 0, then released.
/// Ref: https://doc.rust-lang.org/std/net/struct.TcpListener.html#method.bind
fn free_port() -> u16 {
    TcpListener::bind("0.0.0.0:0")
        .and_then(|l| l.local_addr())
        .unwrap_or_else(|e| panic!("free port: {e}"))
        .port()
}

#[tokio::test]
async fn http_answers_on_loopback_and_refuses_on_the_hosts_own_address() {
    let Some(host_ip) = non_loopback_ip() else {
        eprintln!("skipped: this host has no non-loopback address to dial");
        return;
    };
    let port = free_port();
    let ct = CancellationToken::new();
    let server = McpServer::new(Arc::new(FakeBackend::default()));
    let serving = tokio::spawn(serve_http(server, port, ct.clone()));

    let loopback = SocketAddr::new(MCP_LOOPBACK, port);
    let mut up = false;
    for _ in 0..300 {
        if TcpStream::connect(loopback).await.is_ok() {
            up = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(up, "serve_http never answered on {loopback}");

    // Refused at once when nothing listens there; a timeout counts as refused too.
    let beyond = SocketAddr::new(host_ip, port);
    let dialed = tokio::time::timeout(Duration::from_secs(2), TcpStream::connect(beyond)).await;
    assert!(
        !matches!(dialed, Ok(Ok(_))),
        "{beyond} accepted a connection: MCP HTTP is reachable beyond this device"
    );

    ct.cancel();
    let stopped = serving.await.unwrap_or_else(|e| panic!("serve task: {e}"));
    assert!(stopped.is_ok(), "serve_http: {stopped:?}");
}
