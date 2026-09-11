# MCP server: tools, resources, prompts (plan M6, plan §6)

Goal. One `rmcp` server exposing the §6.3 tools, §6.4 resources/prompts, plus
`todo_notes_get {id}` / `todo_notes_set {id, text}`, with `file` args accepting a ref path.

## Surface (design §6.3)
- Tools: `todo_list`, `todo_search`, `todo_get`, `todo_add`, `todo_complete`,
  `todo_uncomplete`, `todo_edit`, `todo_move`, `todo_delete`, `todo_archive`,
  `todo_batch`, `todo_history`, `todo_raw` — arg names exactly as the §6.3 table.
- Add `todo_notes_get {id}` and `todo_notes_set {id, text}` (plan M6) → daemon
  `GetNotes`/`EditNotes` (crdt-notes-doc, M5).
- `file` params take a ref path (`q4-roadmap/todo.txt`), resolved by the daemon's walker,
  not a bare filename.
- `todo_batch` declares `dry_run`; the diff rendering lands in its own task.
- Structured errors `{ code, message, line?, spec_rule? }` (§6.3) — one error type,
  carried through every handler.

## Resources + prompts (design §6.4)
- `todotxt://todo.txt`, `todotxt://done.txt`, `todotxt://task/{id}`,
  `todotxt://project/{name}`, `todotxt://context/{name}`, `todotxt://history?since=…`.
- Prompts: `plan_today`, `weekly_review`, `triage_inbox`.

## Placement
- `txtodo-mcp` owns schemas + the `rmcp` skeleton (may depend only on txtodo-proto,
  txtodo-query per budgets.json). `txtodo-daemon` imports it and supplies handlers —
  the daemon is the only thing behind the tools (design §6.1).

## Library
- rmcp: https://docs.rs/rmcp

## Acceptance
- Every §6.3 tool and §6.4 resource/prompt is registered with the documented schema.
- Scope (write:*) is enforced from the token's caveats, not re-implemented per handler.
