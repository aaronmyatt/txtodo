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

## As built (2026-09-13, agent)

`transport.rs` matches the plan: `serve_stdio`/`serve_http` (both taking the one `McpServer` from
`schema.rs`), the constants (`MCP_PORT` = 8636, `MCP_PATH` = "/mcp", `MCP_SERVICE` =
"_txtodo-mcp._tcp", `MCP_LOOPBACK`/`MCP_LAN`), and `advertise_lan` via `mdns-sd` (the same crate
`sync-lan-transport`'s `discovery.rs` uses, so one mDNS library in the workspace).

### Deviations, with reasons

- **The daemon does not host the HTTP transport itself.** The plan says
  "`txtodo-daemon/src/serve.rs` starts the HTTP server on `MCP_PORT`". Instead, `txtodo-mcp` ships
  its own binary (`main.rs`, `--dir --stdio|--http [--lan] [--token]`) that dials `txtodod`'s
  existing unix socket via `GrpcMcpBackend` and hosts *both* transports itself — `txtodo mcp --http`
  starts the HTTP listener, `txtodo mcp --stdio` the stdio one, both as their own process, both
  reusing the exact same gRPC path. Reasons: (1) `txtodo-daemon`'s `serve.rs`/`main.rs` are already
  a fair amount of surface (socket lifecycle, pidfile, watcher, SIGTERM draining) and this task is
  meant to be additive, not a daemon-internals change; (2) it means exactly one `McpBackend`
  implementation (`GrpcMcpBackend`) instead of a second, direct, in-process one living inside
  `txtodo-daemon` that duplicates actor-state access; (3) design §6.1's actual invariant — "the
  daemon is the only thing behind them" — holds either way, since every tool call still ends at
  `txtodod` over gRPC. The tradeoff: HTTP isn't automatically available whenever `txtodod` is
  running; a human/agent must separately run `txtodo mcp --http` (or a service manager entry, not
  built here) to expose it. Revisit if that manual step turns out to matter in practice.
- **`txtodo-cli` execs a sibling binary instead of linking `txtodo-mcp`.** `budgets.json`'s
  `allowedDeps` grants the `txtodo-mcp` dependency edge only to `txtodo-daemon`, not `txtodo-cli`.
  `crates/txtodo-cli/src/commands/mcp.rs` is therefore a thin process launcher — the same
  "binary beside this one, else PATH" pattern `commands::service::txtodod_path` already uses for
  `txtodod` — that execs the `txtodo-mcp` binary (`src/main.rs`, alongside `src/lib.rs` in the same
  crate) and inherits stdio, so `--stdio`'s JSON-RPC framing passes through untouched. `--lan
  --stdio` is rejected by the CLI before spawning anything; `--lan` without `--token` is refused by
  the `txtodo-mcp` binary itself (closer to the actual bind).
- **`--token` is not a verified bearer token.** Real token verification is
  [mcp-auth](../mcp-auth/notes.md)/[mcp-tokens](../mcp-tokens/notes.md)'s job — out of scope here.
  `--token <id>` is currently just an opaque string attached to `ApplyRequest.agent.token_id`
  (`name` fixed to `"mcp"`) for op-log attribution; the `--lan`-needs-`--token` check is a cheap,
  meaningful guard against the worst case (an unauthenticated LAN-reachable daemon) but is not a
  substitute for real auth.

### Verified live (not just unit-tested)

Ran a real `txtodod` against a scratch workspace and drove `txtodo mcp` directly (not the fake
in-process backend `tests/smoke.rs` uses):

- `txtodo mcp --stdio`, piped a real `initialize` → `notifications/initialized` →
  `tools/list` → `tools/call todo_list` session: got back the workspace's actual task, correctly
  parsed (priority, dates, `+work` project), then a clean exit on stdin EOF.
- `txtodo mcp --http`: `curl -X POST http://127.0.0.1:8636/mcp` (SSE response) round-tripped the
  same `initialize`; `curl GET /mcp` returned 200 (session-ready stream); `curl GET /other`
  returned 404.
- `txtodo mcp --http --lan` without `--token`: refused with the documented message, exit 1.
- `txtodo mcp --stdio --lan`: rejected as a usage error, exit 1.
- A second `txtodo mcp --http` while one was already bound: failed naming port 8636 and asking
  "is another txtodod already serving MCP on port 8636?", exit 1.

Not built/verified here (explicitly out of scope): the MCP SDK reference client smoke test
(`tests/smoke.rs` uses a bare `()` `ClientHandler` over an in-memory duplex pipe, not the real SDK
client, per this task's "a lightweight self-test is fine" note) — that is
[mcp-smoke-test](../mcp-smoke-test/notes.md); the mDNS TXT record's port field and
`_txtodo-mcp._tcp.local.` resolution were exercised by `transport::tests::constants_match_the_design`
and code review only, not a live two-machine LAN discovery (no second machine available here).

## References

- Design §6.1 (transports, 8636, `_txtodo-mcp._tcp`, LAN option).
- Plan §6 (M6 goal; "Smoke test with a real MCP client … over both transports").
- `rmcp` stdio: <https://docs.rs/rmcp/latest/rmcp/transport/stdio/> · HTTP:
  <https://docs.rs/rmcp/latest/rmcp/transport/http/>
- `mdns-sd`: <https://docs.rs/mdns-sd>
- [../sync-lan-transport/notes.md](../sync-lan-transport/notes.md) (shared mDNS crate).
