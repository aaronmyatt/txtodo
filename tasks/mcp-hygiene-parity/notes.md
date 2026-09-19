# mcp-hygiene-parity

## As built (2026-09-20)

- `todo_lint {file?, workspace?}` returns `[{line, finding}]`, the shape `txtodo lint --json` prints.
- The MCP crate may not link `txtodo-core`, so the findings come from a new read-only `Lint` RPC that
  runs `txtodo_core::lint_findings`, moved out of the CLI, see git log for "lint_findings".
  The CLI calls the same function.

## Held

- `todo_fmt` is not added: it is the one command that rewrites lines it was not asked about, and it
  waits until agent-principal attribution covers a whole-file rewrite.
