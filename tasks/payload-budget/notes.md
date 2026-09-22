# payload-budget

## Goal

Give `budgets.json` a real `payloadKB` (it is `null`) by bounding what one MCP/HTTP response can
carry. Split out of the M6 security review on 2026-09-20: it limits response size, not who can call,
so it is a robustness item, not a security finding. Low priority. It came from three lines in
`tasks/security-m6-review` (pin `MAX_LINE_BYTES`, derive `payloadKB`, update `stack.md`).

## What bounds each response today

| response | cap today | worst case |
|---|---|---|
| `todo_list` rows | `limit` is client-chosen, "0/absent = daemon default" (`backend_args.rs`); the daemon's own ceiling is not verified | unbounded |
| history ops | `HISTORY_MAX_LIMIT = 1000`, summary 60 chars plus about 100 bytes fixed | about 160 KiB |
| whole-file resource | `MAX_LINES_PER_FILE = 1_000_000`, no per-line cap | unbounded |

## Proposal (each number is an assumption; change them)

- Cap each returned line at 4096 bytes. A line is one line of human text and the advisory hint is
  100 characters. Cut on a character boundary and mark the line truncated.
- Cap `todo_list` at 50 rows per call, and say when there are more. A row is the raw line plus its
  parsed fields; I assume at most twice the line, so 8 KiB worst case and 400 KiB for 50 rows.
- Page the whole-file resource at 512 KiB.
- Then `payloadKB = 512`. `budgets.json` and `stack.md` change together. `budgets.json` is a frozen
  path; `.claude/UNFROZEN` exists.

## Changed from the old plan

The M6 plan pinned `MAX_LINE_BYTES` in `txtodo-core`. Here the cap is on the response, in
`txtodo-mcp`, and writes are never refused. A hard cap in core would reject lines a user typed,
which is a product rule nobody asked for.

## Known gaps

- Whether the daemon has its own ceiling on `todo_list`'s `limit` was not checked.
- The row-size assumption (twice the line) is not measured against the real response shape.

## As built (2026-09-23)

- Line cap: `doc.rs::MAX_ROW_BYTES = 4096`, applied in `FileDoc::row` after parsing, so
  `priority`/`projects`/`kv` still reflect the whole line; `TaskRow.truncated` marks the cut.
- `todo_list`: `tools_read::MAX_LIST_ROWS = 50`, `ListArgs.offset`. The backend still returns
  every filtered row (resources and prompts depend on that); the tool pages at its boundary and
  appends a second text block only when rows were left out. `limit` above 50 clamps.
- File resource: `resources::MAX_RESOURCE_BYTES = 512 KiB`; `todotxt://<file>?offset=N`; the
  reply's second `ResourceContents` has the next page's URI and how many bytes remain.
- `payloadKB = 512` in `budgets.json`; `.claude/stack.md` names the three constants.
- Known gaps: `project/{name}`, `context/{name}` and `history` resources are not paged (history
  is bounded by `HISTORY_MAX_LIMIT`); the daemon's own `todo_list` ceiling was still not checked
  since the MCP side no longer relies on it; row size was not measured against real JSON.
