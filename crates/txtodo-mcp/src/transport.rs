//! `txtodo mcp --stdio` and Streamable HTTP on `127.0.0.1:8636/mcp` (mcp-transports notes.md).
//! One [`crate::schema::McpServer`], two entry points.
//!
//! The MCP surface is reachable from this device only (task mcp-local-only, decided 2026-09-20).
//! Stdio has no network. HTTP binds [`MCP_LOOPBACK`] and no flag widens it: `--lan`, the
//! every-interface bind and the `_txtodo-mcp._tcp` mDNS advertisement are gone. Loopback alone is
//! not private: any browser tab can send a request to `127.0.0.1`, and a hostile page can rebind a
//! name to it. So the HTTP service also checks the `Host` and `Origin` headers ([`http_router`]).

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;

use rmcp::ServiceExt;
use rmcp::transport::io::stdio;
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::streamable_http_server::{StreamableHttpServerConfig, StreamableHttpService};
use tokio_util::sync::CancellationToken;

use crate::schema::McpServer;

/// §6.1: "8636 spells TODO on a phone keypad".
pub const MCP_PORT: u16 = 8636;
/// The Streamable HTTP path; never the default, always set explicitly.
pub const MCP_PATH: &str = "/mcp";
/// The only address the HTTP transport binds.
pub const MCP_LOOPBACK: IpAddr = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1));
/// `Host` values the HTTP service answers (with or without the port): the DNS-rebinding guard. A
/// page that rebinds `evil.example` to 127.0.0.1 still sends `Host: evil.example`, and gets 403.
pub const LOOPBACK_HOSTS: [&str; 3] = ["127.0.0.1", "localhost", "::1"];

/// Browser origins the HTTP service answers: only a page served from this server's own address.
/// A request with no `Origin` (every non-browser MCP client) passes; any other origin gets 403.
/// Origins compare as (scheme, host, port), RFC 6454: https://www.rfc-editor.org/rfc/rfc6454#section-5
pub fn loopback_origins(port: u16) -> Vec<String> {
    vec![
        format!("http://127.0.0.1:{port}"),
        format!("http://localhost:{port}"),
        format!("http://[::1]:{port}"),
    ]
}

/// Why a transport stopped before a client asked it to.
#[derive(Debug)]
pub enum TransportError {
    /// The `rmcp` session failed to initialize.
    Init(String),
    /// The stdio session task panicked or was aborted.
    Join(tokio::task::JoinError),
    /// `addr`'s port was already bound — "is another txtodod running?" (same shape `doctor`
    /// reports for port availability).
    PortInUse(SocketAddr, std::io::Error),
    /// The HTTP server itself failed after binding.
    Http(std::io::Error),
}

impl std::fmt::Display for TransportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TransportError::Init(e) => write!(f, "mcp session: {e}"),
            TransportError::Join(e) => write!(f, "mcp stdio task: {e}"),
            TransportError::PortInUse(addr, e) => write!(
                f,
                "cannot bind {addr} ({e}); is another txtodod already serving MCP on port {MCP_PORT}?"
            ),
            TransportError::Http(e) => write!(f, "mcp http server: {e}"),
        }
    }
}

impl std::error::Error for TransportError {}

/// Serves `server` over stdin/stdout until the peer disconnects.
/// <https://docs.rs/rmcp/latest/rmcp/transport/io/fn.stdio.html>
pub async fn serve_stdio(server: McpServer) -> Result<(), TransportError> {
    let running = server
        .serve(stdio())
        .await
        .map_err(|e| TransportError::Init(e.to_string()))?;
    running.waiting().await.map_err(TransportError::Join)?;
    Ok(())
}

/// rmcp's Streamable HTTP service nested at [`MCP_PATH`], answering only a loopback `Host` and
/// an absent or loopback `Origin` (403 otherwise). rmcp does the checks; this names the lists
/// rather than leaning on its defaults, whose `Origin` list is empty, which means "not checked".
/// <https://docs.rs/rmcp/latest/rmcp/transport/streamable_http_server/struct.StreamableHttpServerConfig.html>
pub fn http_router(server: McpServer, port: u16, ct: CancellationToken) -> axum::Router {
    // `StreamableHttpServerConfig` is `#[non_exhaustive]`, so build the default and set fields
    // rather than use a struct-update literal.
    let mut config = StreamableHttpServerConfig::default();
    config.cancellation_token = ct;
    config.allowed_hosts = LOOPBACK_HOSTS.iter().map(|h| (*h).to_owned()).collect();
    config.allowed_origins = loopback_origins(port);
    let service: StreamableHttpService<McpServer, LocalSessionManager> = StreamableHttpService::new(
        move || Ok(server.clone()),
        Arc::new(LocalSessionManager::default()),
        config,
    );
    axum::Router::new().nest_service(MCP_PATH, service)
}

/// Serves `server` over Streamable HTTP at `http://127.0.0.1:{port}{MCP_PATH}` until `ct` is
/// cancelled. The address is not a parameter: nothing can make this listen beyond loopback.
/// <https://docs.rs/rmcp/latest/rmcp/transport/streamable_http_server/>
pub async fn serve_http(
    server: McpServer,
    port: u16,
    ct: CancellationToken,
) -> Result<(), TransportError> {
    let addr = SocketAddr::new(MCP_LOOPBACK, port);
    let router = http_router(server, port, ct.clone());
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .map_err(|e| TransportError::PortInUse(addr, e))?;
    axum::serve(listener, router)
        .with_graceful_shutdown(async move { ct.cancelled_owned().await })
        .await
        .map_err(TransportError::Http)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constants_match_the_design() {
        assert_eq!(MCP_PORT, 8636);
        assert_eq!(MCP_PATH, "/mcp");
        assert!(
            MCP_LOOPBACK.is_loopback(),
            "the only bind address is loopback"
        );
        assert!(loopback_origins(MCP_PORT).contains(&"http://127.0.0.1:8636".to_owned()));
    }
}
