//! `txtodo mcp --stdio` and Streamable HTTP on `127.0.0.1:8636/mcp` (mcp-transports notes.md).
//! One [`crate::schema::McpServer`], two entry points; `--lan` (bind + mDNS) is two independent
//! facts a caller composes: bind [`MCP_LAN`] instead of [`MCP_LOOPBACK`], and call
//! [`advertise_lan`].

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;

use mdns_sd::{ServiceDaemon, ServiceInfo};
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
/// mDNS service type `--lan` advertises (design §6.1). Bare, RFC 6763 form — [`advertise_lan`]
/// appends the `.local.` domain mdns-sd needs to register it.
pub const MCP_SERVICE: &str = "_txtodo-mcp._tcp";
/// Default bind: loopback only.
pub const MCP_LOOPBACK: IpAddr = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1));
/// `--lan` bind: every interface.
pub const MCP_LAN: IpAddr = IpAddr::V4(Ipv4Addr::UNSPECIFIED);

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

/// Serves `server` over Streamable HTTP at `http://{addr}{MCP_PATH}` until `ct` is cancelled.
/// <https://docs.rs/rmcp/latest/rmcp/transport/streamable_http_server/>
pub async fn serve_http(
    server: McpServer,
    addr: SocketAddr,
    ct: CancellationToken,
) -> Result<(), TransportError> {
    // `StreamableHttpServerConfig` is `#[non_exhaustive]`, so build the default and mutate the one
    // field this server cares about rather than a struct-update literal.
    let mut config = StreamableHttpServerConfig::default();
    config.cancellation_token = ct.clone();
    let service: StreamableHttpService<McpServer, LocalSessionManager> = StreamableHttpService::new(
        move || Ok(server.clone()),
        Arc::new(LocalSessionManager::default()),
        config,
    );
    let router = axum::Router::new().nest_service(MCP_PATH, service);
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .map_err(|e| TransportError::PortInUse(addr, e))?;
    axum::serve(listener, router)
        .with_graceful_shutdown(async move { ct.cancelled_owned().await })
        .await
        .map_err(TransportError::Http)
}

/// Advertises `_txtodo-mcp._tcp` on `port` via mDNS (`--lan`'s second fact). The same `mdns-sd`
/// crate `crates/txtodo-sync/src/discovery.rs` uses for `_txtodo._udp`, so the workspace has one
/// mDNS library, not two. The TXT record repeats `port` explicitly (mDNS's SRV record already
/// carries it) so a phone finds the right endpoint even if a future build changes the default.
pub fn advertise_lan(host_name: &str, port: u16) -> Result<ServiceDaemon, mdns_sd::Error> {
    let daemon = ServiceDaemon::new()?;
    let service_type = format!("{MCP_SERVICE}.local.");
    let props: [(&str, String); 1] = [("port", port.to_string())];
    let info = ServiceInfo::new(&service_type, "txtodo-mcp", host_name, "", port, &props[..])?
        .enable_addr_auto();
    daemon.register(info)?;
    debug_assert!(!MCP_SERVICE.is_empty(), "the service type is never empty");
    Ok(daemon)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constants_match_the_design() {
        assert_eq!(MCP_PORT, 8636);
        assert_eq!(MCP_PATH, "/mcp");
        assert_eq!(MCP_SERVICE, "_txtodo-mcp._tcp");
        assert_ne!(MCP_LOOPBACK, MCP_LAN);
    }
}
