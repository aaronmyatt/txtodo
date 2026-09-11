# Smoke test with the SDK reference MCP client over both transports (plan M6, plan §6)

Plan M6: "Smoke test with a real MCP client (the SDK's reference client) over both transports."
Design §6.1 defines the two transports; §6.6 is the example session.

## What "smoke" means here

Not the scope matrix or the acceptance tests — those are exhaustive. The smoke test proves one
thing: a real client (the SDK's reference client, e.g. rmcp's client) completes the handshake and
exercises a few tools over **both** transports, end to end. The SDK is rmcp
(<https://docs.rs/rmcp>); transports per
<https://modelcontextprotocol.io/specification/2025-06-18/basic/transports>.

Per transport:

- stdio: spawn `txtodo mcp --stdio`, speak MCP over its stdin/stdout; the token comes from
  `--token` / `default_stdio_token`.
- Streamable HTTP: run the server on a test port, connect with the `Authorization: Bearer` header.

## The calls to smoke

`initialize` → `tools/list` → `resources/list` → `todo_list` (seeded tasks come back with `id`,
`line`, parsed fields, `raw`) → `todo_add` (appears in the file, attributed to the token principal)
→ `resources/subscribe` on `todotxt://todo.txt` + one external edit yields
`notifications/resources/updated` → `todo_batch {dry_run: true}` returns the unified diff without
applying.

## Hermetic

Temp `todo_dir` and an ephemeral port; no reliance on a live daemon on 8636. A missing or tampered
Bearer token over HTTP is refused, and stdio without a token fails auth the same way.
