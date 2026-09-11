# 0005 — Use the official Rust MCP SDK (rmcp) with stdio and Streamable HTTP

- Status: accepted
- Date: 2026-09-11
- Deciders: project owner (plan §1, decision 005; do not relitigate)

## Context
Agents are first-class (design §0). MCP is a moving spec; hand-rolling JSON-RPC and transport framing would chase it forever.

## Decision
We will use `rmcp` (verify crate name and version before adding; plan §1 #5) for `txtodo-mcp`, exposing stdio (`txtodo mcp --stdio`) and Streamable HTTP on `127.0.0.1:8636/mcp`.

## Consequences
- Good: protocol conformance comes from the SDK; both transports from one server.
- Bad: SDK release cadence dictates ours for protocol updates.
- Neutral / follow-ups: see the plan milestone that lands it.

## Alternatives considered
- Hand-rolled JSON-RPC: every spec revision is a rewrite.
- HTTP only: local agents that spawn a subprocess need stdio.
