# Agent principal + quarantine on every MCP mutation (plan M6, plan §6)

## Goal

Every mutation an MCP client makes flows through the daemon's `Apply` as
`Principal::Agent { token_id, name, device }` — never `Principal::User`, never a direct file
write (design §6.1: "the daemon is the only thing behind them"). The token's `quarantine=@ctx`
caveat appends its context (default `@inbox`, per token, design §6.5) to `todo_add` lines only.

## Design

### The principal already exists — the work is wiring, not inventing

`crates/txtodo-daemon/src/convert.rs::parse_principal` already maps the proto
`AgentPrincipal { token_id, name }` to `Principal::Agent` and enforces `name` is `1..=64` bytes:

```rust
// crates/txtodo-daemon/src/convert.rs (exists, M3)
pub fn parse_principal(agent: Option<pb::AgentPrincipal>, device: DeviceId)
    -> Result<Principal, Status>;
// None => Principal::User { device }; Some(a) => Principal::Agent { token_id, name, device }
```

`Principal::Agent`'s `Display` (model `op.rs`) renders `agent:{name}@{device}` — exactly what
`txtodo blame` must show. The MCP task fills `ApplyRequest.agent` from the *verified* token
(mcp-auth's `AuthContext`), never from anything the client claims.

### The MCP mutation path

`txtodo-daemon` implements `txtodo_mcp::McpBackend` (the trait from mcp-server-tools). Every
mutating method builds an `ApplyRequest` with `agent` set, and quarantine is applied to `Add` lines
before the request is built:

```rust
// crates/txtodo-daemon/src/mcp_backend.rs — implements txtodo_mcp::McpBackend
async fn add(&self, text: String, file: Option<RefPath>) -> Result<TaskRow, McpError> {
    let mut line = text;
    // design §6.5: quarantine appends its context to agent-added lines, default @inbox
    if let Some(ctx) = &self.auth.quarantine {
        line.push(' ');
        line.push_str(ctx);                     // + one extra context, nothing else
    }
    let req = ApplyRequest {
        path: resolve(file).to_string(),        // walker.rs::relative, same as mcp-server-tools
        mutations: vec![Mutation { kind: Some(mutation::Kind::Add(pb::Add { line })) }],
        agent: Some(pb::AgentPrincipal {
            token_id: self.auth.token_id.to_string(),
            name: self.auth.name.clone(),
        }),
    };
    self.workspace.apply(req).await.map_err(McpError::from)
}
```

- `Apply` (in `handle.rs`) stamps `created:` + `id:` exactly as it does for the user path — the
  quarantine context is the only difference. The MCP layer never stamps dates or ids itself.
- Complete/edit/delete/move build the same `agent` field but apply **no** quarantine tag — the
  caveat is add-only by design §6.5.

### Attribution is already stored

The op log stores `principal` as text (model `op.rs` + store `ops.principal`), and
`convert.rs::to_summary` surfaces it in `OpSummary.principal`. `txtodo blame` reads the op log, so
an MCP add is attributed the moment `Apply` commits it — no separate attribution table.

## Placement/dependencies

- `crates/txtodo-daemon/src/mcp_backend.rs` (the `McpBackend` impl — new module, or the serve-side
  wiring already sketched in mcp-server-tools). Depends on `txtodo-mcp` (allowed: daemon may import
  it) and reuses `convert.rs`, `handle.rs`, `walker.rs`.
- No proto change: `ApplyRequest.agent` and `AgentPrincipal` already exist
  (`crates/txtodo-proto/proto/txtodo/v1/txtodo.proto`).

## Edge cases & invariants

- **No MCP path writes the file directly.** Invariant: the MCP backend calls only `Apply`/`Watch`/
  `GetFile`/`History` on the workspace; a grep-style test asserts no `fs::`/`write` call in
  `mcp_backend.rs`.
- `token_id`/`name` come from `AuthContext` (the verified token), never from the request body.
- Quarantine context is validated when the caveat is parsed (mcp-tokens: `@`-prefixed); the backend
  appends it verbatim, never re-interprets it.
- Quarantine is add-only: an `edit`/`complete`/`delete`/`move` of a quarantined line does not
  re-tag it.
- A token *without* a `quarantine` caveat adds lines untagged — quarantine is opt-in per token
  (design §6.5).
- Blank-line and non-`Add` mutations still carry `agent`; only the quarantine append is conditional.

## Acceptance

- A quarantined `todo_add` produces a line with the extra context *and* the daemon-stamped
  `created:`/`id:` — and nothing else differs from a user add.
- The op for that mutation carries `Principal::Agent`; `txtodo blame` on the line shows
  `agent:<name>@<device>`, not `you@<device>`.
- A non-add mutation (e.g. `todo_complete`) carries the agent principal but no quarantine tag.
- A token with no quarantine caveat adds untagged lines (the default-`@inbox` append happens only
  when the caveat is present).

## References

- plan M6 (txtodo-implementation-plan.md), design §6.1/§6.2/§6.5 (txtodo-design.md)
- principal types: `crates/txtodo-model/src/op.rs`, `crates/txtodo-daemon/src/convert.rs`
- backend trait: [../mcp-server-tools/notes.md](../mcp-server-tools/notes.md) · auth: [../mcp-auth/notes.md](../mcp-auth/notes.md)
