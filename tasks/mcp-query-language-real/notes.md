# mcp-query-language-real

## As built (2026-09-20)

- `crates/txtodo-mcp/src/parse.rs::matches_query` replaces the `matches_minimal_query` stub: the same
  matching as `txtodo list TERM...` (whitespace-split terms, every term must match, case-insensitive
  substring, a leading `-` excludes). `todo_list`'s `query` and `todo_search` both use it, so MCP
  search agrees with CLI search. Test vectors mirror `txtodo-cli`'s `commands::list::matches`.

## Known gaps

- The stub's `not done` / `done` tokens are gone; they are now plain substring terms.
- The design §8 query language (`txtodo-query`) is still a stub; this only makes MCP match the CLI.
