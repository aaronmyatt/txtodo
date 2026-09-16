# txtodo-mcp

## Purpose
MCP tools, resources, prompts, capability tokens. Plan M6.

## Public interface
Tools per design §6.3, resources §6.4, `token` module.

## Invariants
- Every mutation goes through the daemon's Apply with Principal::Agent.
- Logs carry ids, counts and hashes — never line text, tokens or payloads.
- May depend only on: txtodo-proto, txtodo-query.
