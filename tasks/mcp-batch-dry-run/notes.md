# todo_batch dry_run returns a unified diff, structured errors carry line and spec_rule (plan M6, plan §6)

Plan M6: "`dry_run` on `todo_batch` returns a unified diff against the projection. Structured
errors: `{ code, message, line?, spec_rule? }`." Design §6.3 lists `todo_batch {ops[], dry_run}`
as atomic and §6.6 shows the exact diff shape the agent sees. This task ships both halves: the
diff renderer and the error type — one task because they share the same "apply ops to a throwaway
state" path.

## Design

`todo_batch` declares `ops: Vec<ToolOp>, dry_run: bool` where `ToolOp` is the MCP-side alias of the
proto `Mutation` oneof (`add`/`complete`/`edit`/`move`/`delete`, `crates/txtodo-proto/proto/txtodo/v1/txtodo.proto`).
The dry-run path must never touch the real projection: clone the document, apply, diff, discard.

```rust
// crates/txtodo-daemon/src/mcp/batch.rs  — the daemon owns state, txtodo-mcp owns the schema
pub struct BatchDiff {
    pub applied: u32,   // ops that would append
    pub diff: String,   // unified diff, §6.6 shape
}

/// Clone the projection, replay `ops` through DocState, diff before/after bytes.
pub fn dry_run(
    contents: &Contents,          // handle.rs: bytes + hash of the current projection
    ops: &[Mutation],             // proto Mutation oneof, converted in convert.rs
    now: Date,                    // injected clock — Complete stamps `today` (ADR 0011)
) -> Result<BatchDiff, McpError>;
```

Apply is `DocState::apply(&mut self, kind: &OpKind) -> Result<(), StateError>`
(`crates/txtodo-daemon/src/state.rs:157`) — "on `Err` the state is unchanged", so a clone is a
safe sandbox: replay each converted op onto a fresh `DocState`, re-render with the same
`File::to_bytes` path the actor uses, then `txtodo_core::diff_lines(before, after)` (shipped M1,
`tasks/core-diff`) and format hunks as

```
--- todo.txt
+++ todo.txt
@@ -4 +4,5 @@
-(B) 2026-09-08 Draft Q4 roadmap +work due:2026-09-12 id:01J9K3…
+(A) 2026-09-08 Draft Q4 roadmap +work due:2026-09-12 id:01J9K3…
+2026-09-11 Review roadmap draft +work @laptop due:2026-09-14 id:01J9M7…
```

`diff_lines` keys by `Key::Id(Ulid)` when the line has an `id:` tag, else content — id keys are
what keep hunks stable and produce `Move` instead of delete+insert across a reorder.

## Structured errors

```rust
// crates/txtodo-mcp/src/error.rs — serialized to {code, message, line?, spec_rule?}
pub struct McpError {
    pub code: McpErrorCode,              // closed enum, one variant per failure
    pub message: String,                 // states what was attempted and with which values
    pub line: Option<u32>,               // 1-indexed, blanks included (CLI convention)
    pub spec_rule: Option<&'static str>, // e.g. "specs/todotxt.abnf §priority"
}
```

Rides the MCP JSON-RPC error `data` field (https://modelcontextprotocol.io/specification/2025-06-18/basic/messages)
so a client that only reads `code`/`message` still works.

- `line` is the line **after** the preceding ops in the batch, not the on-disk line. A batch that
  edits line 4 then adds above it must report the shifted line for the second op.
- `spec_rule` points at the exact `specs/todotxt.abnf` rule the line violates — priority case,
  date placement, tag grammar — so an agent producing `(a) task` learns why (design §6.3).

## Placement/dependencies

- `txtodo-mcp` declares the `todo_batch` tool schema and `McpError`; its `allowedDeps` are
  `txtodo-proto`, `txtodo-query` only (`.claude/budgets.json`).
- `txtodo-daemon` imports `txtodo-mcp` and supplies the handler — "the daemon is the only thing
  behind them" (design §6.1). The `dry_run` fn above lives in `txtodo-daemon` because only it may
  hold a `DocState`; `txtodo-core` provides `diff_lines`.

## Edge cases & invariants

- **Atomic.** One invalid op rejects the whole batch with that structured error; nothing is
  applied and the real projection is untouched.
- **No side effects.** Assert the negative: a dry run performs no `handle.apply` call, no file
  write, no op-log append. The full-stack "hash unchanged" assertion lives in
  [mcp-acceptance-tests](../mcp-acceptance-tests/notes.md); here it is structural.
- **Bounds.** Reuse `MAX_MUTATIONS_PER_APPLY = 10_000` (`mutation.rs`) as the batch cap, asserted
  before applying — an over-cap batch is refused outright. Diff output is bounded by
  `MAX_LINES_PER_FILE = 1_000_000` and `MAX_PROJECTION_BYTES = 64 MiB` (store).
- **Exhaustiveness.** `ToolOp → Mutation → OpKind` conversions are closed matches with no default
  arm (constitution §3); a new op kind must not silently fall through to a no-op.
- **`Complete` needs the injected clock.** `Mutation::Complete` stamps the client's `today`; a dry
  run must take `now` as a parameter or the diff for "mark done" is wrong by a day.

## Acceptance

- `dry_run: true` on the §6.6 example returns that unified diff, byte-for-byte hunks, and the
  file's hash is unchanged.
- `(a) task` add returns `code` + `line` + `spec_rule` naming the priority-case rule.
- A batch that edits-then-adds reports the shifted line for the second op's error.
- One invalid op in a 3-op batch rejects all three; no partial apply.
- An over-cap batch is refused with a distinct error before any op is converted.

## References

- MCP messages/errors: https://modelcontextprotocol.io/specification/2025-06-18/basic/messages
- rmcp (tool + error plumbing): https://docs.rs/rmcp
- Myers diff in core: https://neil.fraser.name/writing/diff/myers.pdf (see `tasks/core-diff`)
