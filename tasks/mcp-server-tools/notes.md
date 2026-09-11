# rmcp server: §6.3 tools, §6.4 resources/prompts, plus todo_notes_get/set (plan M6, plan §6)

## Goal

One `rmcp` server exposing every §6.3 tool, every §6.4 resource/prompt, plus
`todo_notes_get {id}` / `todo_notes_set {id, text}` (plan M6), with `file` args accepting a ref
path (`q4-roadmap/todo.txt`). Structured errors `{ code, message, line?, spec_rule? }` (§6.3).
The server is schemas + a handler trait; the daemon implements the trait — design §6.1: "the
daemon is the only thing behind them."

## Design

### The handler seam — keeps `txtodo-mcp` inside its allowedDeps

`budgets.json` `allowedDeps["txtodo-mcp"] = ["txtodo-proto", "txtodo-query"]`. No store, no daemon,
no watcher, no tantivy. So `txtodo-mcp` owns the MCP surface (schemas, the `rmcp` skeleton, the
handler trait); `txtodo-daemon` — which *may* import `txtodo-mcp` — implements the trait against
real state. This is the same shape the daemon already uses with `txtodo-proto`.

```rust
// txtodo-mcp/src/lib.rs — the trait the daemon implements; schemas live beside it.
#[async_trait]
pub trait McpBackend: Send + Sync {
    async fn list(&self, args: ListArgs) -> Result<Vec<TaskRow>, McpError>;
    async fn search(&self, text: String) -> Result<Vec<TaskRow>, McpError>;
    async fn get(&self, target: GetTarget) -> Result<TaskRow, McpError>;   // id OR line
    async fn add(&self, text: String, file: Option<RefPath>) -> Result<TaskRow, McpError>;
    async fn complete(&self, id: TaskId, done: bool) -> Result<TaskRow, McpError>;
    async fn edit(&self, id: TaskId, patch: FieldPatch) -> Result<TaskRow, McpError>;
    async fn move_task(&self, id: TaskId, anchor: MoveAnchor) -> Result<TaskRow, McpError>;
    async fn delete(&self, id: TaskId, confirm: bool) -> Result<(), McpError>;
    async fn archive(&self, file: RefPath) -> Result<ArchiveOutcome, McpError>;
    async fn batch(&self, ops: Vec<TodoOp>, dry_run: bool) -> Result<BatchOutcome, McpError>;
    async fn history(&self, since: Option<Hlc>, id: Option<TaskId>) -> Result<Vec<OpSummary>, McpError>;
    async fn raw_read(&self, file: RefPath, lines: Vec<u32>) -> Result<Vec<String>, McpError>;
    async fn raw_write(&self, file: RefPath, line: u32, text: String) -> Result<(), McpError>;
    async fn notes_get(&self, id: TaskId) -> Result<String, McpError>;
    async fn notes_set(&self, id: TaskId, text: String) -> Result<(), McpError>;
}
```

`TaskRow` = `{ id, line, raw, priority?, due?, created?, completed?, done, projects[], contexts[],
kv[] }` — the parsed fields §6.3 promises, plus `raw` (the untouched line text).

### Tool table (design §6.3 — arg names are the contract, do not rename)

| Tool | Args | Notes |
|---|---|---|
| `todo_list` | `query`, `file`, `limit` | query language §8 via `txtodo-query`; returns `TaskRow`s |
| `todo_search` | `text` | full-text over the tantivy index — owned by the daemon backend |
| `todo_get` | `id` \| `line` | single task |
| `todo_add` | `text`, `file` | `text` is a raw line minus dates; daemon stamps `created:` + `id:` |
| `todo_complete` / `todo_uncomplete` | `id` | `pri:` preservation per spec |
| `todo_edit` | `id`, `patch` | `patch` = `{ priority?, due?, append?, replace? }` — field-level, never a whole-line rewrite |
| `todo_move` | `id`, `before` \| `after` | reorder |
| `todo_delete` | `id`, `confirm` | tombstone; `confirm` must be `true` |
| `todo_archive` | `file` | move completed tasks to `done.txt` |
| `todo_batch` | `ops[]`, `dry_run` | atomic; `dry_run` declares only — diff rendering is [mcp-batch-dry-run](../mcp-batch-dry-run/notes.md) |
| `todo_history` | `since`, `id` | reads the op log |
| `todo_raw` | `file`, `lines[]` (read) / `line`, `text` (write) | needs the `raw` scope |
| `todo_notes_get` | `id` | → daemon gRPC `GetNotes` (M5) |
| `todo_notes_set` | `id`, `text` | → daemon gRPC `EditNotes` (M5) |

### The two added tools (plan M6)

`todo_notes_get`/`todo_notes_set` map to the M5 gRPC pair `GetNotes`/`EditNotes` in
`crates/txtodo-proto/proto/txtodo/v1/txtodo.proto` — see [crdt-notes-doc](../crdt-notes-doc/notes.md).
The notes file is `notes.md` inside the task's `ref:` directory; the daemon resolves `id` → ref dir,
never the client.

### `file` args take a ref path

`file` is resolved by the daemon's walker (`crates/txtodo-daemon/src/walker.rs::walk` /
`relative`), not treated as a bare filename. A bare `todo.txt` still means the workspace root file;
`q4-roadmap/todo.txt` means the sub-list. Normalization happens once, in the daemon backend, at the
boundary.

### Structured errors

One error type carried through every handler, mapped into MCP's error result (JSON-RPC `-32000`
range) by the `rmcp` skeleton:

```rust
pub struct McpError { pub code: &'static str, pub message: String, pub line: Option<u32>, pub spec_rule: Option<&'static str> }
```

`spec_rule` points at a rule id in `specs/todotxt.abnf` / `specs/ref-directories.md` so an agent
that produces `(a) task` learns why (§6.3).

## Placement/dependencies

`crates/txtodo-mcp/src/` gains `schema.rs` (tool/resource/prompt declarations + `rmcp` macros),
`backend.rs` (the trait + `TaskRow`/arg types), `tools.rs`, `resources.rs`, `prompts.rs`,
`error.rs`. Each file ≤ 400 lines, each fn ≤ 60 lines (budgets.json). New deps: `rmcp`
(<https://docs.rs/rmcp>), `async-trait` — both need human sign-off and a `cargo deny check` pass
before any code lands. `txtodo-daemon` wires the `McpBackend` impl in its `serve.rs`.

## Edge cases & invariants

- Scope (`write:*`, `raw`, …) is enforced from the token's caveats **once**, before dispatch —
  never re-implemented per handler (that is [mcp-auth](../mcp-auth/notes.md) + [mcp-tokens](../mcp-tokens/notes.md)).
- `todo_delete`/`todo_raw` write/`todo_archive` require `confirm: true` — assert the arg, don't trust.
- `todo_get` by `line` vs by `id`: the daemon rejects when both are given and they disagree (the
  `TaskRef` staleness check already does this for `Apply`).
- `todo_add` text with a `created:` already present is a client error, not silently re-stamped.
- The registered set is exhaustive — an unlisted tool means a schema bug, caught by the acceptance test.

## Acceptance

- Every §6.3 tool and §6.4 resource/prompt is registered with the documented name and args, plus
  `todo_notes_get`/`todo_notes_set` — and nothing else (the set is an exact match).
- `todo_notes_get {id}` returns the `notes.md` bytes for that task's ref dir; `todo_notes_set`
  writes them and emits a `NotesEdit` op.
- `file: "q4-roadmap/todo.txt"` resolves to the sub-list; `file: "todo.txt"` resolves to root.
- A handler error for `(a) task` carries `spec_rule` pointing at the paren/priority rule.
- `todo_add` with a hand-written `created:` tag is rejected, not re-stamped.

## References

- Design §6.3 (tools table), §6.4 (resources/prompts), §6.1 (daemon behind the surface).
- Plan §6 (M6 goal + acceptance).
- `rmcp`: <https://docs.rs/rmcp> · MCP spec: <https://spec.modelcontextprotocol.io/>
- `crdt-notes-doc`: [../crdt-notes-doc/notes.md](../crdt-notes-doc/notes.md) (GetNotes/EditNotes).
