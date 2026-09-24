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

## As built
