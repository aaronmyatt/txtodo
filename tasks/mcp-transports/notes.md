# MCP transports: stdio and Streamable HTTP (plan M6, plan §6)

Goal. Same MCP server behind two transports (design §6.1): stdio and Streamable HTTP on
`127.0.0.1:8636/mcp`, plus `--lan` advertising over mDNS.

## Transports
- stdio: `txtodo mcp --stdio` — one JSON-RPC stream over stdin/stdout, for local agents.
- Streamable HTTP: `http://127.0.0.1:8636/mcp` (8636 spells TODO on a keypad, §6.1).
- `--lan`: bind `0.0.0.0` and advertise `_txtodo-mcp._tcp` via mdns-sd (same crate as
  sync-lan-transport, M4), so a desktop agent finds the daemon on a phone.

## Placement
- The daemon (`txtodod`) hosts the server; it alone may import txtodo-mcp and reach the
  daemon state. `txtodo mcp --stdio` is the CLI launcher that attaches stdio to it.
  Exact launcher wiring per plan, TBD when started.
- Constants named with units: `MCP_PORT=8636`, `MCP_PATH="/mcp"`,
  `MCP_SERVICE="_txtodo-mcp._tcp"`.

## Library
- rmcp stdio + HTTP transports: https://docs.rs/rmcp
- mdns-sd: https://docs.rs/mdns-sd

## Acceptance
- Smoke test with the SDK reference MCP client over both transports (plan M6).
- `--lan` binds 0.0.0.0 and the mDNS TXT/service record resolves.
