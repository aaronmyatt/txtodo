# Tests: quarantine line and blame show the agent, dry run leaves the hash unchanged, rate limit pauses the token (plan M6, plan §6)

Plan M6 acceptance — the three observable behaviours the milestone closes on:

- Quarantined `todo_add` produces a line with the extra context and `txtodo blame` shows the agent
  principal.
- Dry run leaves the file's hash unchanged.
- Rate-limit test pauses a token and emits the event.

## Quarantine (design §6.5)

A token with a `quarantine=@ctx` caveat gets `@ctx` appended to every `todo_add` line; default
`@inbox`. The test adds a line through a quarantined token and asserts the written line carries the
context, and that a non-quarantined token appends nothing. This is the §6.5 "human triages them"
path made checkable.

## Blame (design §6.2)

Every mutation is recorded in the op log with the token's principal (`Principal::Agent`), so
`txtodo blame <line>` answers "which agent" from the log, not from a bare "agent" label. Test: after
an agent add, `blame` names the token principal.

## Dry run hash

Hash the file bytes (including the trailing newline) before and after `todo_batch {dry_run: true}`.
Assert byte-identical. Same guarantee as the mcp-batch-dry-run unit test, asserted at the
full-stack level here.

## Rate limit (plan M6)

60 mutations/min/token and 10 deletes/min/token; exceeding pauses the token and emits a
notification event. The two budgets are independent. Use the injected fake clock (CLAUDE.md §7 — no
sleeps, deterministic), drive 61 mutations (or 11 deletes), assert the token is paused, the next
mutation is denied, and the event names the token and the exceeded limit. A paused token must not
pause a second token, and the pause lifts after the window.
