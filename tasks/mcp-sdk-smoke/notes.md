# mcp-sdk-smoke

## Goal

Proof that a stock MCP client works against txtodo over both stdio and Streamable HTTP. Filed
2026-09-20 to come back to; nothing here is started.

## Status

- Waiting on one approval: rmcp's client features as dev-dependencies of `txtodo-mcp`. They are
  test-only and never in the shipped binary. Recommended: approve. It is an approval, not a design
  choice. Not given yet; the first line stays `@human` and so does the root line, so the backlog
  loop skips the whole ref until you clear both.

## Design

- Use the SDK's own client, not a hand-rolled one, so a protocol drift shows up as a failure.
- Stdio: spawn the server as a child process and talk to it. HTTP: bind `127.0.0.1` (the server is
  loopback-only, see `mcp-local-only`) and connect the client over Streamable HTTP.
- Each test runs against a real daemon on an isolated socket, not a fake backend, and calls one read
  tool and one write tool.
- Keep it in the normal test run so it fails loudly when the client cannot connect.

## Known gaps

- Not checked yet: which rmcp features the client needs and whether they pull new transitive
  dependencies past `deny.toml`. The dependency line finds out.

## As built (2026-09-24)

- `crates/txtodo-mcp/tests/sdk_client.rs`, two `#[ignore]`d tests (CI-only, like every sibling
  real-daemon test). Each starts `txtodod --dir <ws> --no-lan --no-relay` in a short `/tmp` dir
  (socket path under 100 chars), lists tools, calls `todo_add`, then `todo_list` finds the line.
- Stdio: spawns the real `txtodo-mcp --stdio` binary and hands its stdout/stdin to rmcp's client.
  Also checks the binary exits 0 once the client cancels.
- HTTP: `serve_http` in-process on a free loopback port, rmcp's `StreamableHttpClientTransport`.
- Dev-deps: rmcp `transport-streamable-http-client-reqwest` (reqwest was already in the lockfile,
  no TLS turned on; 0 new crates) and tokio `process`/`time`. `cargo deny check` green.

Known gaps:
- The `--http` binary's own startup is untested: it always binds port 8636, so a test can't pick one.
- Only the `--dir` daemon shape is tested; `--global` and no-flag are not.
