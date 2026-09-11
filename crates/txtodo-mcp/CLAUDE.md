# txtodo-mcp

## Purpose
MCP tools, resources, prompts, capability tokens. Plan M6.

## Public interface
Tools per design §6.3, resources §6.4, `token` module.

## Invariants
- Every mutation goes through the daemon's Apply with Principal::Agent.
- May depend only on: txtodo-proto, txtodo-query.
