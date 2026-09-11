# Tests: quarantine line and blame show the agent, dry run leaves the hash unchanged, rate limit pauses the token (plan M6, plan §6)

Plan M6 acceptance — the three observable behaviours the milestone closes on: quarantined
`todo_add` carries the extra context and `blame` shows the agent principal; dry run leaves the
hash unchanged; exceeding a rate limit pauses the token and emits the event. These are full-stack
tests (daemon + MCP + store + CLI), not unit tests — the same guarantees are asserted structurally
in [mcp-batch-dry-run](../mcp-batch-dry-run/notes.md), [mcp-rate-limit](../mcp-rate-limit/notes.md)
and [mcp-agent-principal](../mcp-agent-principal/notes.md).

## Quarantine (design §6.5)

A token with a `quarantine=@ctx` caveat gets `@ctx` appended to every `todo_add` line; default
`@inbox`. This is the "human triages them" path made checkable:

- add through a quarantined token → the written line ends with the context tag (e.g.
  `… buy 400 rubber ducks +work @inbox`);
- add through a token with `quarantine=@custom` → `@custom` is appended;
- add through a token with no quarantine caveat → nothing is appended;
- quarantine is add-only: complete/edit/delete on a quarantined line are not re-tagged.

## Blame (design §6.2)

Every mutation is recorded in the op log with the token's principal — proto
`AgentPrincipal { token_id, name }` mapped by `daemon/src/convert.rs::parse_principal` to
`Principal::Agent { token_id, name, device }` — so `txtodo blame <line>` answers "which agent"
from the log (principal string `agent:name@device`), not a bare "agent" label.

- after an agent add, `blame` names the token principal;
- the op-log row (proto `OpSummary.principal`) carries `agent:name@device`, not `you@dev`.

## Dry run hash

Hash the file bytes — including the trailing newline — before and after
`todo_batch { dry_run: true }`. Assert byte-identical. `Hash = [u8; 32]` comes back on
`Contents`/`ApplyResponse` (`daemon/src/expected.rs`), so compare hashes directly, not re-hashed
bytes.

## Rate limit (plan M6)

60 mutations/min/token and 10 deletes/min/token — two independent buckets; exceeding either pauses
the token and emits a notification event (design §6.5, [mcp-rate-limit](../mcp-rate-limit/notes.md)).
Use the injected fake clock (constitution §7 — seeded PRNG, fake time, no sleeps):

- drive 61 mutations in the window → token paused, the 61st is denied, the event names the token
  and the exceeded limit;
- drive 11 deletes → the separate delete budget trips independently of the mutation budget;
- a paused token does not pause a second token;
- advancing the fake clock past the 60 s window lifts the pause.

## Placement/dependencies

- `crates/txtodo-daemon/tests/` — the existing suite (`grpc.rs`, `editor_saves.rs`,
  `external_edits.rs`, `crash.rs`) already spins a daemon + store in a temp dir; these tests reuse
  `tests/support/mod.rs` the same way.
- The MCP transport under test is the in-process `txtodo-mcp` server driven directly, so these
  tests stay hermetic — no `127.0.0.1:8636`, no live daemon, no real clock.

## Acceptance

- Quarantined add produces a line with the extra context; non-quarantined appends nothing.
- `blame` on the added line shows the agent principal from the op log.
- `todo_batch { dry_run: true }` leaves the file hash byte-identical (trailing newline included).
- 61st mutation / 11th delete pauses the token, denies the next call, and emits the event.
- The pause is per-token and lifts after the window on a fake-clock advance.

## References

- rmcp: https://docs.rs/rmcp · macaroon: https://docs.rs/macaroon
- daemon test support: `crates/txtodo-daemon/tests/support/mod.rs`
