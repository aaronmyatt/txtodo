# Security checklist review before M6 close and re-justify the payloadKB budget (plan M6, plan §6)

Plan M6: "Security checklist review before M6 close and re-justify the payloadKB budget." The
checklist is one line of prose (M4/M6/M8 gate), carried forward in
[security-m4-review](../security-m4-review/notes.md).

## What M6 makes testable

M4 deferred two items because they were not buildable yet. Both are now in scope:

- **MCP HTTP refuses non-loopback unless `--lan`** — transports shipped. Test: bind is loopback by
  default; `--lan` is required to bind `0.0.0.0`.
- **tokens never logged** — tokens shipped. Test: capture `tracing` across `token create`, auth,
  and a tool call; assert no token value, root secret, or caveat appears in the JSON logs.

Also re-check the inherited items rather than assuming M4's results still hold: secrets in logs, the
root secret living only in the keystore, and the slug-validator fuzz result after M5's ref work
(re-run the fuzz target for a bounded session).

## payloadKB re-justification

`budgets.json.payloadKB` is `null` with the note "Re-justify when `txtodo-mcp` lands" — the M0-M5
rationale was "no HTTP surface". That surface exists now (Streamable HTTP on 8636). Re-justify with
a concrete number derived from the existing caps — `MAX_LINES_PER_FILE` × max line width for file
resources, plus the `todo_list` `limit` and `history` paging — not a number picked out of the air.
`budgets.json` is a frozen path: change it via `/setup`, which shows the diff before writing.

## Deliverable

A dated M6 section in `RATCHET.md` listing each checklist item as pass / fail / deferred-to-M8,
each with the test that proves it. The relay op-type-leak item stays deferred to M8 with a todo.txt
line in that milestone.
