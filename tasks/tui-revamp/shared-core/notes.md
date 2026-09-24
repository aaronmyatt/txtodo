# shared-core

## Goal
- Pure logic both clients need is written once, in `txtodo-core`.
  - The TUI links it directly. Desktop reaches it through the `txtodo-ffi` wasm build.

## Design
- `query::matches`:
  - Terms are AND-ed, case-insensitive substrings. `-term` excludes. `is:open` / `is:done`.
  - Must equal `txtodo list` and `crates/txtodo-mcp/src/parse.rs::matches_query` today.
  - One golden table drives the core, CLI and MCP tests.
  - The richer design §8 grammar stays in `txtodo-query`, which is still a stub. This line does not build it.
- `lint::strict_hints`: the three messages from the c2 mockup `todotxt.js:308-313`.
- `edit::toggle_priority` / `toggle_complete_text` / `insert_chip`: the prompt-bar chip behaviour (`todotxt.js:283-302`).
- `universal::due_bucket(today)` and `universal::group`: the Universal page's groups (`c2/universal.js:20-27`).
- wasm exports follow `crates/txtodo-ffi/src/{parse_check,diff_view}.rs`: plain Rust, tested on host, with a thin `wasm.rs` shim.
- Fence: core, ffi, cli and mcp each get their own session.

## As built (2026-09-25)
- `txtodo_core::query::{matches, matches_terms, GOLDEN}`. `txtodo list` uses `matches_terms` (a
  quoted phrase stays one term, as in todo.sh); MCP reaches it through a `txtodo-query` re-export,
  since `budgets.json` lets MCP depend on `txtodo-query` but not on `txtodo-core`. CLI and MCP each
  test against `GOLDEN`. `is:open`/`is:done` are new in both.
- `txtodo_core::strict_hint::strict_hint` (not `lint::strict_hints`): the first of the mockup's
  three hints, or `None`; checked row by row against the mockup's own regexes in node.
- `txtodo_core::chips` (not `edit::*`): `Chip`, `apply_chip`, `toggle_priority`, `insert_chip`,
  `toggle_complete_text`, desktop's `editPopoverLogic.ts` tests mirrored. Byte carets.
- `txtodo_core::universal`: `days_between`, `DueBucket`/`due_bucket`, `due_label` (the row badge),
  `GroupBy`, `RowFacts`, `group_name`, `group` (headings with row indices, in the mockup's order).
- `txtodo-ffi`: `shared.rs` (UTF-16 carets, row shaping, host-tested) under seven new wasm exports.
  `apps/desktop/src/lib/wasm-core` rebuilt; `wasmCoreShared.test.ts` loads it with `initSync`.

## Known gaps
- Desktop still uses its TS copies (`editPopoverLogic.ts`, the Universal view); switching is the
  `@parity` work in `tasks/desktop-ui-revamp`.
- Group names sort with a lower-case compare, not `localeCompare`: accented names may order
  differently from the mockup.
