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

## As built (2026-09-13, agent)

Crate layout matches the plan closely, plus two files it didn't anticipate:
`backend.rs` (`McpBackend` trait + every arg/result type — `TaskRow`, `ListArgs`, `GetTarget`,
`FieldPatch`, `MoveArgs`/`MoveAnchor`, `TodoOp`, `Hlc`, `ApplyOutcome`, `OpSummary`, `FileMeta`, plus
one tool-args struct per tool), `error.rs` (`McpError`), `schema.rs` (`McpServer`, the `rmcp`
`ServerHandler`: `#[tool_router]` for all 15 tools, manual overrides for resources/prompts — no
macro exists for those), `tools_read.rs`/`tools_write.rs` (one function per tool, called one-line
each from `schema.rs` so its `#[tool_router]`/`#[tool_handler]` block stays readable),
`resources.rs`, `prompts.rs`, `grpc_backend.rs`/`grpc_read.rs`/`grpc_write.rs`/`grpc_convert.rs`
(`GrpcMcpBackend`, the one `McpBackend` impl — a gRPC client of `txtodod`), `parse.rs` (see
deviation below), `main.rs` (the `txtodo-mcp` binary — see mcp-transports' As-built). Tests:
`tools.rs`'s `json_result` unit-tested implicitly via every module's own tests (21 unit tests
across `parse`/`error`/`grpc_convert`/`grpc_read`/`grpc_write`/`transport`/`resources`/`prompts`)
plus `tests/smoke.rs` (in-process client/server over a duplex pipe, 3 tests: exact tool-set,
one real tool-call round trip, resources+prompts). Verified again live against a real `txtodod`
(see mcp-transports' As-built) — `todo_list` over real stdio and real HTTP both returned the
actual parsed task from a real `todo.txt`.

Registered set (exact match, verified by `tests/smoke.rs`): `todo_list`, `todo_search`, `todo_get`,
`todo_add`, `todo_complete`, `todo_uncomplete`, `todo_edit`, `todo_move`, `todo_delete`,
`todo_archive`, `todo_batch`, `todo_history`, `todo_raw`, `todo_notes_get`, `todo_notes_set`.
Resources: `todotxt://todo.txt`, `todotxt://<any synced todo/done path>` (listed), plus templates
`todotxt://task/{id}`, `todotxt://project/{name}`, `todotxt://context/{name}`,
`todotxt://history{?since}` (read-only; no subscription push — that's
[mcp-resource-subs](../mcp-resource-subs/notes.md)). Prompts: `plan_today`, `weekly_review`,
`triage_inbox`, each embedding real backend data.

### Deviations, with reasons

- **`parse.rs` replaces `txtodo-query`.** The design's query language (§8) isn't built yet —
  `txtodo-query` is still the empty stub from scaffolding, a separate task. `txtodo-mcp` also may
  not depend on `txtodo-core` (`budgets.json.slices.allowedDeps`), so `TaskRow`'s parsed fields
  (priority/dates/projects/contexts/kv) can't come from the real parser either. `parse.rs` is a
  minimal, self-contained todo.txt line parser/editor (tested) that fills both gaps: `todo_list`'s
  `query` supports `+project`, `@context`, `done`/`not done`, and bare substrings (conjunction
  only); `todo_search` is a case-insensitive substring match, not the tantivy full-text index §6.3
  describes as "owned by the daemon backend" (no such index exists anywhere in the daemon yet).
  Replace this module wholesale once `txtodo-query` lands.
- **`todo_move` has no daemon equivalent.** The design wants a same-file reorder by anchor task
  (`before`/`after`). The only `Move` mutation the daemon exposes relocates a task *across files*
  (plan M7) via a destination path, not a position — `apply_route.rs` also refuses a `Move` next to
  any other mutation in one `Apply` call. The tool is registered with the right schema (satisfies
  "every tool registered"), but its handler returns a clear `McpError` naming the gap rather than
  faking a reorder. This is the one place the task's "add an RPC only if genuinely needed, and note
  it" applied: a proper fix needs a new daemon op (e.g. a `Reorder` mutation with a same-document
  anchor), out of scope for a tools-wrapper task.
- **`todo_archive` is not atomic and doesn't blank-collapse.** One `Apply(Move)` per completed
  task (highest line first, so earlier line numbers never shift out from under a still-pending
  call) — `apply_route.rs` refuses more than one `Move` per batch, so there is no single atomic
  call available. It also doesn't collapse the blank lines a completed-and-moved line leaves
  behind, unlike the CLI's local `archive`; `daemon_mode.rs`'s own comment already flags that
  specific cleanup as inexpressible via intent-level mutations today.
- **`todo_batch` executes sequentially, not atomically.** Each op reuses its single-tool
  counterpart (`todo_add`, `todo_edit`, ...); `ApplyOutcome` reports only a count, not a real
  per-op hash/hlc. `dry_run: true` never calls `Apply` at all (returns `applied: 0`, no diff) —
  diff rendering is [mcp-batch-dry-run](../mcp-batch-dry-run/notes.md)'s job, not this task's.
- **`todo_notes_set` still attributes to the local user.** `notes.rs::edit_notes_impl` hardcodes
  `Principal::User` (a pre-existing M5 gap, confirmed by reading it, not introduced here) — an
  agent's notes edits aren't yet attributed to the agent. Presumably fixed by
  [mcp-agent-principal](../mcp-agent-principal/notes.md).
- **`GrpcMcpBackend` is a gRPC client everywhere**, including the daemon-hosted HTTP transport
  (mcp-transports' As-built explains why): one `McpBackend` implementation, reusing only RPCs
  `txtodo-cli`'s `client.rs` already proves out, instead of a second copy of daemon-internal state
  access living in this crate.

Acceptance checked: the tool/resource set is an exact match (`tests/smoke.rs`); `file:
"q4-roadmap/todo.txt"` vs `"todo.txt"` both pass straight through to the daemon's own path
resolution (never resolved client-side, per the design above); `todo_add` rejects a leading date or
`id:` tag (`grpc_write_tests::validate_add_text_rejects_a_leading_date_or_id_tag`); `todo_delete`
asserts `confirm` before ever calling the backend. Not separately re-verified: a handler error for
`(a) task` carrying `spec_rule` — no tool currently rejects malformed priority syntax itself (the
daemon's own line parser would reject it inside `Apply`, surfaced as `McpError::daemon`, not yet
carrying a `spec_rule`); a real gap worth a follow-up unit test once `todo_edit`/`todo_add`
validation grows spec-rule-aware error mapping.

## References

- Design §6.3 (tools table), §6.4 (resources/prompts), §6.1 (daemon behind the surface).
- Plan §6 (M6 goal + acceptance).
- `rmcp`: <https://docs.rs/rmcp> · MCP spec: <https://spec.modelcontextprotocol.io/>
- `crdt-notes-doc`: [../crdt-notes-doc/notes.md](../crdt-notes-doc/notes.md) (GetNotes/EditNotes).
