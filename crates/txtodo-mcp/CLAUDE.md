# txtodo-mcp

## Purpose
MCP tools, resources, prompts, capability tokens. Plan M6.

## Public interface
Tools per design §6.3, resources §6.4, `token` module.

## Invariants
- Every mutation goes through the daemon's Apply with Principal::Agent.
- Logs carry ids, counts and hashes — never line text, tokens or payloads.
- May depend only on: txtodo-proto, txtodo-query, txtodo-telemetry, txtodo-daemon-launch (matches
  `.claude/budgets.json`'s `allowedDeps.txtodo-mcp`). `main.rs` calls
  `txtodo_daemon_launch::ensure_daemon` before dialing the socket (task daemon-always-available);
  `grpc_backend.rs`/`global_socket.rs` stay pure and still don't import that crate.
