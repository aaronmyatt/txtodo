# 0010 — Fixed ports, service names and paths (txtodo naming)

- Status: accepted
- Date: 2026-09-11
- Deciders: project owner (plan §1, decision 010; do not relitigate)

## Context
Docs, tests and integrations need stable names. The project was renamed from Sisyphus to txtodo at /setup (2026-09-11); the plan text was updated in the same change.

## Decision
We will fix: MCP HTTP on `127.0.0.1:8636`; gRPC on the local socket only; metrics on `127.0.0.1:8637`; mDNS services `_txtodo._udp` (sync) and `_txtodo-mcp._tcp` (MCP); daemon binary `txtodod`, CLI `txtodo`, TUI `txtodo-tui`; config at `$XDG_CONFIG_HOME/txtodo/config.toml`; state at `<workspace>/.txtodo/`; env `TXTODO_TODO_DIR`.

## Consequences
- Good: tests and docs can hard-code them; 8636 spells TODO on a keypad.
- Bad: a rename later touches every doc and ADR (this one included).
- Neutral / follow-ups: see the plan milestone that lands it.

## Alternatives considered
- Dynamic ports: discovery needed for local clients.
- Keeping the Sisyphus names: the crate and binary names already changed; mixed naming is worse than either.
