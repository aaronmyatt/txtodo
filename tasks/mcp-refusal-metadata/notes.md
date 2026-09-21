# mcp-refusal-metadata

Found reviewing the last 100 commits (`18b37ea^..HEAD`) on 2026-09-21.

## Goal

`4754867` made refusals carry the offending line and the spec rule, so an agent gets something
actionable instead of a sentence. The daemon emits it. The MCP layer only half receives it.

## The gaps

- `crates/txtodo-mcp/src/grpc_write.rs:39-50` allow-lists **three hardcoded rule strings**. The
  daemon owns the list (`crates/txtodo-daemon/src/mutation.rs:189-198`,
  `MutationError::spec_rule`) and `error.rs`'s own test already uses a `"priority"` rule that is
  not among the three. Any rule the daemon adds is dropped on the floor and the agent sees a
  refusal with no `spec_rule`. Making `McpError::spec_rule` a `String` instead of a
  `&'static str` — or sharing one const list — removes the whole class.
- `crates/txtodo-mcp/src/grpc_hygiene.rs:21-23` and `crates/txtodo-mcp/src/grpc_notes.rs:12` each
  have their own private `status()` copy; only `grpc_write.rs`'s reads the metadata.
  `conflicts_resolve` and the notes edits go through the same `Apply`/mutation path and can return
  the same statuses carrying `x-txtodo-error-line` / `x-txtodo-error-rule`, so those two tools give
  a bare message where `todo_edit` gives structured data. Both should delegate to
  `grpc_write::status`.
- No test covers the mapping at all. `apply_dry_run.rs:222` asserts the *daemon* emits the
  metadata; nothing asserts MCP *surfaces* it. That is why the allow-list gap went unnoticed.

Small, self-contained, and the kind of thing that silently rots — three copies of `status()` is
already two too many.
