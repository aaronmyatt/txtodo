# 0028 — MCP is reachable from this device only

- Status: accepted
- Date: 2026-09-20
- Deciders: project owner

## Context
Design §6.1 and ADR 0010 gave the MCP HTTP transport an opt-in LAN mode: `--lan` bound every
interface and advertised `_txtodo-mcp._tcp` over mDNS, "so an agent on your desktop can find the
daemon on your phone". The only guard was that `--lan` refused to start without `--token`, and
that token was never checked on a request. The M6 security review draft
(`tasks/security-m6-review/findings-draft.md`, F1) read that as an unauthenticated MCP on every
interface. The open backlog line asked which bearer-token check to build to defend it.

## Decision
We will not defend `--lan`; we remove it. The MCP server is only ever reachable from this device.

- Stdio has no network.
- Streamable HTTP binds `127.0.0.1:8636` and no flag or config binds another address.
  `transport::serve_http` takes a port, not an address.
- Nothing is advertised: the `_txtodo-mcp._tcp` service type of ADR 0010 is dropped. `_txtodo._udp`
  (sync discovery) is unchanged.
- Loopback is not private on its own, since any browser tab can send a request to `127.0.0.1` and a
  hostile page can rebind its name to it. The HTTP service answers only a loopback `Host` and an
  absent or same-server `Origin`, and gives 403 otherwise.
- No bearer auth is built for MCP. `--token` stays: it names the agent principal on a mutation.

## Consequences
- An agent on another machine cannot reach this device's MCP server. It reaches the same tasks
  through sync, on its own device's daemon.
- A browser-based MCP client served from another local port (MCP Inspector) is refused by the
  `Origin` check. There is no `--allow-origin` yet.
- Every local user of a shared machine can still reach `127.0.0.1:8636`. A unix socket would be
  per-user, but stock MCP clients speak stdio or an HTTP URL.
- ADR 0010 is superseded only in its `_txtodo-mcp._tcp` clause.

## Alternatives considered
- Bearer auth checked against the daemon's own tokens (a new verify RPC): real work to defend a
  mode nobody needs today.
- A shared secret in the config file: a second credential store beside the token table.
