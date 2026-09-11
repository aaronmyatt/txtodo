# MCP smoke test with the SDK reference client over stdio + Streamable HTTP (plan M6, plan §6)

## Goal

One integration test drives the SDK's reference MCP client — `rmcp`'s client, since `txtodo-mcp`
is built on `rmcp` (<https://docs.rs/rmcp>) — through the full handshake and a few tools over
**both** transports (design §6.1). This proves the seam end to end; the exhaustive scope matrix and
per-tool tests live in mcp-server-tools, mcp-auth, mcp-tokens. Plan M6: "Smoke test with a real MCP
client (the SDK's reference client) over both transports."

## Design

### Hermetic by construction

- A `tempfile::tempdir()` todo_dir, an ephemeral HTTP port (`127.0.0.1:0`, read the assigned port),
  and a token minted in-test from an in-memory keystore — no live daemon on `8636`, no real keychain
  (same discipline as sync-keystore's in-memory `KeyStore`).
- stdio: spawn `txtodo mcp --stdio --token <token> --dir <dir>` as a child, drive `rmcp`'s stdio
  transport over its stdin/stdout; a `Drop` guard kills + reaps the child so the test never leaks a
  daemon.
- HTTP: start the server in-process on the ephemeral port, connect `rmcp`'s Streamable-HTTP client
  with `Authorization: Bearer <token>`.

```rust
// crates/txtodo-daemon/tests/mcp_smoke.rs — has rmcp transitively via txtodo-mcp
async fn stdio_round(dir: &Path, token: &str) {
    let child = Command::new(env!("CARGO_BIN_EXE_txtodo"))
        .args(["mcp", "--stdio", "--token", token, "--dir", dir.to_str().unwrap()])
        .stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap();
    let client = stdio_transport(child);            // rmcp client
    let init = client.initialize(InitRequest).await.unwrap();   // handshake
    let tools = client.list_tools().await.unwrap();             // §6.3 set present
    // …todo_list / todo_add / subscribe / todo_batch dry_run, same as http_round…
}

async fn http_round(dir: &Path, token: &str) {
    let port = bind_ephemeral();                    // 127.0.0.1:0
    let client = http_transport(format!("http://127.0.0.1:{port}/mcp"), token).await; // Bearer header
    // …same calls…
}
```

### The calls to smoke (design §6.6 is the reference session)

1. `initialize` → handshake succeeds on both transports.
2. `tools/list` → the §6.3 tool set (plus the two notes tools) is present; `resources/list` → the
   six `todotxt://` resources from §6.4.
3. `todo_list` on seeded tasks → rows come back with `id`, `line`, parsed fields, and `raw`.
4. `todo_add` → the line appears in the file and is attributed to the token principal
   (`agent:<name>@<device>`, via mcp-agent-principal).
5. `resources/subscribe` on `todotxt://todo.txt`, then one external file edit → the client receives
   `notifications/resources/updated` (design §6.4).
6. `todo_batch { dry_run: true }` → returns the unified diff, and the file's hash is unchanged
   (plan M6: "Dry run leaves the file's hash unchanged").
7. Auth negative: a missing/tampered Bearer over HTTP is refused (401); stdio without a token fails
   auth the same way.

## Placement/dependencies

- `crates/txtodo-daemon/tests/mcp_smoke.rs` — `txtodo-daemon` may import `txtodo-mcp` (and thus
  `rmcp`), and owns the server + workspace needed to stand up both transports in-process.
- Dev-deps: `tempfile` (already idiomatic per stack.md), `rmcp`'s client (transitively present).
  No new workspace deps expected; if a direct dev-dep is needed it still requires sign-off + deny.

## Edge cases & invariants

- **No sleeps.** Readiness is a bounded poll on the transport handshake / health, not a fixed delay
  (stack.md: deterministic, injected clock).
- The child process is killed and reaped via a guard on every exit path, including panics.
- The transport is the only variable: both rounds run the same call list, so a difference between
  stdio and HTTP is the bug the test names.
- `resources/updated` is asserted by receiving an event after an *external* edit — this exercises the
  `Watch` wiring, not just the tool RPCs.
- The auth negative proves the smoke client isn't bypassing mcp-auth (a client that "works" without
  a token would be a red flag).

## Acceptance

- Both transports complete `initialize`, `tools/list`, `resources/list`, `todo_list`, `todo_add`.
- `todo_add` over the smoke client lands in the file and is attributed to the token principal.
- `resources/subscribe` delivers `notifications/resources/updated` after an external edit.
- `todo_batch { dry_run: true }` returns the diff and the file hash is unchanged.
- Missing/tampered Bearer → 401 on HTTP; missing token → auth failure on stdio. Test runs hermetic
  (temp dir, ephemeral port, no `8636`).

## References

- plan M6 (txtodo-implementation-plan.md), design §6.1/§6.4/§6.6 (txtodo-design.md)
- rmcp: <https://docs.rs/rmcp> · MCP transports: <https://modelcontextprotocol.io/specification/2025-06-18/basic/transports>
- tools under test: [../mcp-server-tools/notes.md](../mcp-server-tools/notes.md) · auth: [../mcp-auth/notes.md](../mcp-auth/notes.md)
