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
