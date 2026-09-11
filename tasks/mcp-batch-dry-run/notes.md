# todo_batch dry_run returns a unified diff and structured errors carry line and spec_rule (plan M6, plan §6)

Plan M6: "`dry_run` on `todo_batch` returns a unified diff against the projection. Structured
errors: `{ code, message, line?, spec_rule? }`." Design §6.3 lists `todo_batch {ops[], dry_run}` as
atomic; §6.6 shows the exact diff shape an agent sees.

## Dry run must never touch the projection

`dry_run: true` applies the ops to a clone of `DocState` and diffs the before/after bytes, then
discards the clone. The real projection is untouched — the full-stack "hash unchanged" assertion
lives in [mcp-acceptance-tests](../mcp-acceptance-tests/notes.md); here the rule is simply that no
`Apply` op leaves the MCP server on a dry run.

Diff via `txtodo_core::diff_lines` (shipped at M1, `tasks/core-diff`), rendered as a unified diff
with `--- todo.txt` / `+++ todo.txt` and `@@` hunks, matching the §6.6 example in shape.
Line-level diff by id, falling back to content, is what keeps the hunks stable.

## Structured errors

```rust
struct McpError { code: Code, message: String, line: Option<u32>, spec_rule: Option<&'static str> }
```

- `line` is the 1-indexed line in the file **after** the preceding ops in the batch, not the
  on-disk line. A batch that edits line 4 then adds above it must report the shifted line.
- `spec_rule` points at the exact spec rule the line violates — `specs/todotxt.abnf` for priority
  case, date placement, etc. (design §6.3: an agent producing `(a) task` learns why).
- Atomic: one invalid op rejects the whole batch with that structured error; nothing is applied.

## Bounds

`MAX_BATCH_OPS`, asserted before applying; a larger batch is refused outright. The diff output is
bounded by the file cap (`MAX_LINES_PER_FILE`), so a dry run of a full-file rewrite is capped too.
