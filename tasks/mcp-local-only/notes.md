# mcp-local-only

## Goal

Decided 2026-09-20: the MCP server is only ever reachable from this device. It replaces the old
deferred line "Bearer auth on the MCP HTTP+stdio surface", which asked whether a bearer token is
checked against the daemon's own tokens (A) or a shared secret in config (B). Neither is built. That
line existed to defend the `--lan` case, and `--lan` goes away.

## Design

- Stdio has no network. Streamable HTTP binds `127.0.0.1:8636` (`transport.rs`, already the
  default) and nothing can widen it.
- `--lan` today binds every interface and advertises `_txtodo-mcp._tcp` over mDNS
  (`transport.rs`, `main.rs`, which also refuses `--lan` without `--token`). Remove the flag, the
  bind constant for every interface, the advertisement and that refusal.
- Loopback is not private on its own. Any browser tab can send a request to `127.0.0.1`, and a
  hostile page can rebind a name to it. So the HTTP transport checks the `Host` and `Origin`
  headers and refuses a foreign one.
- The design doc (section 6.1) and the MCP docs still describe `--lan`. They change with the code.

## Known gaps

- Every local user of a shared machine can still reach `127.0.0.1:8636`. A unix socket would be
  per-user, but stock MCP clients speak stdio or an HTTP URL. Not solved here.
- What `--token` does today besides guarding `--lan` is not checked here. Read it before deleting
  the flag.

## As built (2026-09-20)

- `37d7973`: `--lan`, `MCP_LAN`, `MCP_SERVICE`, `advertise_lan` and the `mdns-sd` dependency are
  gone from `txtodo-mcp`. `--lan` now answers a plain refusal. `serve_http` takes a port and binds
  `127.0.0.1`. `http_router` sets rmcp's `allowed_hosts` and `allowed_origins` by name: rmcp 3.3
  checks both, but its default `Origin` list is empty, which means unchecked.
- `tests/http_guard.rs` drives the router with no socket: foreign `Origin` 403, loopback on another
  port 403, rebound `Host` 403, no `Origin` served.
- `--token` stays. It was only a precondition for `--lan`; its real job is the agent principal.
- `42f4574`: ADR 0028, design 6.1, the plan.

Still open:

- The `txtodo-cli` line: `txtodo mcp` still accepts `--lan` and passes it through, where the MCP
  binary now refuses it. Not done here: another session held the tree, so the fence would not hand
  over a second crate.
- A browser client on another local port (MCP Inspector) gets 403. No `--allow-origin`.
