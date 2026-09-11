# MCP transports: stdio + Streamable HTTP on 127.0.0.1:8636/mcp, --lan mDNS (plan M6, plan §6)

## Goal

The *same* `rmcp` server behind two transports (design §6.1): stdio for `txtodo mcp --stdio`, and
Streamable HTTP at `http://127.0.0.1:8636/mcp` (8636 spells TODO on a keypad). `--lan` binds
`0.0.0.0` and advertises `_txtodo-mcp._tcp` over mDNS so a desktop agent finds the daemon on a
phone. The daemon (`txtodod`) is the only thing behind both — §6.1.

## Design

### One server object, two entry points

The server built in [mcp-server-tools](../mcp-server-tools/notes.md) is transport-agnostic. This
task only wires `ServerHandler`/`tool_box` into `rmcp`'s two transports:

```rust
// crates/txtodo-mcp/src/transport.rs — both take the same server, differ only in the IO layer.
pub async fn serve_stdio(server: Server<McpBackend>, token: Token) -> Result<(), McpError>;   // stdin/stdout
pub async fn serve_http(server: Server<McpBackend>, addr: SocketAddr, token: Token) -> Result<(), McpError>;
```

`rmcp` exposes stdio via
`serve_stdio` (<https://docs.rs/rmcp/latest/rmcp/transport/stdio/fn.serve_stdio.html>) and Streamable
HTTP via `serve_http` (<https://docs.rs/rmcp/latest/rmcp/transport/http/index.html>). The HTTP path
must be exactly `/mcp` — set it explicitly, never assume the default.

### Named constants with units

```rust
pub const MCP_PORT: u16 = 8636;              // §6.1: 8636 spells TODO on a phone keypad
pub const MCP_PATH: &str = "/mcp";
pub const MCP_SERVICE: &str = "_txtodo-mcp._tcp";
pub const MCP_LOOPBACK: IpAddr = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1));
pub const MCP_LAN: IpAddr = IpAddr::V4(Ipv4Addr::UNSPECIFIED);   // 0.0.0.0
```

### `--lan` = two independent facts

- Bind `0.0.0.0` instead of `127.0.0.1`.
- Advertise `_txtodo-mcp._tcp` on port `MCP_PORT` via `mdns-sd`
  (<https://docs.rs/mdns-sd>) — the same crate [sync-lan-transport](../sync-lan-transport/notes.md)
  uses, so there is one mDNS dependency, not two.

Loopback-only is the default: a reachable-anywhere MCP daemon with agent tokens on it is a
capability you opt into, exactly like design §6.1 phrases it ("optionally bound to the LAN").

### The CLI launcher

`txtodo mcp --stdio` is a CLI subcommand (`crates/txtodo-cli`), not a daemon flag. It attaches the
stdio transport to a running daemon's state. The daemon hosts the server; the launcher only selects
the transport and passes `--token` through. Wiring details stay thin here — the daemon is the only
thing behind the surface, so `serve_stdio`/`serve_http` call back into `txtodod`'s `McpBackend`
impl, never into a second copy of state.

## Placement/dependencies

- `txtodo-mcp` gains `transport.rs` (both `serve_*` fns + the constants). New deps: `mdns-sd`
  (sign-off + `cargo deny` pass; shared with sync, so the crate is already vetted for the workspace
  if sync-lan-transport landed first).
- `txtodo-daemon/src/serve.rs` starts the HTTP server on `MCP_PORT` and owns the socket lifecycle
  (bind, graceful shutdown on SIGTERM — the pidfile/actor shutdown path already exists).
- `txtodo-cli/src/commands/` gains `mcp.rs` (`mcp --stdio | --http [--lan] [--token <t>]`).
- `txtodo-mcp` may still only depend on `txtodo-proto` + `txtodo-query`; `tokio`/`mdns-sd` are
  transport deps, not store deps.

## Edge cases & invariants

- `--lan` implies HTTP; stdio has no network, so `--lan --stdio` is a usage error, not silently
  ignored.
- Port already bound → a distinct error naming the port (`MCP_PORT`) and "is another txtodod
  running?" — the same shape `doctor` reports for port availability (§1).
- The HTTP server must answer `GET /mcp` with a session-ready response and accept `POST /mcp`;
  other paths 404. No accidental static file serving.
- `0.0.0.0` bind + no auth is the worst combination: `--lan` without a token configured refuses to
  start rather than shipping an open daemon (design §6.2 assumes bearer auth is always on).
- mDNS TXT record carries the port so a phone discovers the right endpoint even on a non-default
  port later; `MCP_SERVICE` is the registered name, asserted non-empty.

## Acceptance

- Smoke test with the MCP SDK reference client over stdio: initialize → `tools/list` → one tool
  call round-trips (plan M6 acceptance).
- Same smoke test over Streamable HTTP on `127.0.0.1:8636/mcp`.
- `--lan` binds `0.0.0.0` and the `_txtodo-mcp._tcp` record resolves to port `8636`.
- `--lan --stdio` is rejected as a usage error.
- Port-conflict start fails with a message naming `8636` and the likely cause.

## References

- Design §6.1 (transports, 8636, `_txtodo-mcp._tcp`, LAN option).
- Plan §6 (M6 goal; "Smoke test with a real MCP client … over both transports").
- `rmcp` stdio: <https://docs.rs/rmcp/latest/rmcp/transport/stdio/> · HTTP:
  <https://docs.rs/rmcp/latest/rmcp/transport/http/>
- `mdns-sd`: <https://docs.rs/mdns-sd>
- [../sync-lan-transport/notes.md](../sync-lan-transport/notes.md) (shared mDNS crate).
