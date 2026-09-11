# MCP per-token rate limits: 60 mutations/min, 10 deletes/min (plan M6, plan §6)

## Goal

Two sliding-window buckets per token — 60 mutations/min and 10 deletes/min. Exceeding either
pauses the token and emits a notification event (design §6.5: "a token that deletes more than N
tasks a minute is paused and the user notified"). The pause survives a daemon restart.

## Design

### The limiter — in `txtodo-daemon`, constants in `txtodo-mcp`

The numbers are part of the MCP surface (design §6.5), so they live once in `txtodo-mcp`; the
limiter needs the store (persist the pause), the clock, and the daemon's event channel, so it lives
in `txtodo-daemon` (which may import `txtodo-mcp`).

```rust
// crates/txtodo-mcp/src/rate.rs — the shared contract, nothing else
pub const MUTATIONS_PER_MIN: u32 = 60;
pub const DELETES_PER_MIN: u32 = 10;
pub const WINDOW_MS: u64 = 60_000;
pub enum OpClass { Mutation, Delete }
```

```rust
// crates/txtodo-daemon/src/rate_limit.rs
pub struct RateLimiter<C: Clock> {
    buckets: BTreeMap<TokenId, TokenWindow>,   // bounded: assert len() <= MAX_TRACKED_TOKENS
    clock: C,                                  // injected; tests use the fixed clock (stack.md idioms)
}
struct TokenWindow {
    mutations: SlidingWindow,                  // 60 per WINDOW_MS
    deletes: SlidingWindow,                    // 10 per WINDOW_MS
}
pub const MAX_TRACKED_TOKENS: usize = 10_000;  // constitution §3: no unbounded collection

pub enum RateDecision {
    Allow,
    Pause { reason: PauseReason },              // 61st mutation / 11th delete
}
pub fn check(&mut self, token: TokenId, op: OpClass) -> RateDecision;
```

- `SlidingWindow` records timestamps (from the injected clock), drops entries older than
  `WINDOW_MS` on each check, and counts the remainder — a rolling 60 s, not fixed buckets.
- `OpClass::Delete` counts `todo_delete` and `todo_archive` (both are `write:delete`, design §6.2
  groups them). Everything else — add, complete, edit, move, raw write, notes — is `Mutation`.
- On `Pause`, the daemon: (1) persists the pause in the store `meta` table
  (`token_paused:<id>` key), so a restart keeps it; (2) emits
  `DaemonEvent::TokenPaused { token_id, name, reason }` on the event channel. The M9 mobile push
  ("claude-code added 3 tasks") is out of scope here — M6 only emits the event (design §6.5).
- Paused tokens reject further mutations until unpaused (human action or TTL). The daemon consults
  the meta table before dispatch, so the pause is sticky, not in-memory.
- Eviction: when `buckets.len() == MAX_TRACKED_TOKENS`, the least-recently-active window is evicted
  before inserting a new token (assert the bound in `insert`, not just in prose).

## Placement/dependencies

- `crates/txtodo-mcp/src/rate.rs`: the three constants + `OpClass` only (no deps).
- `crates/txtodo-daemon/src/rate_limit.rs`: `RateLimiter`, `TokenWindow`, `SlidingWindow`,
  `RateDecision`, `PauseReason`. Uses the daemon's `clock.rs` `Clock` trait and the store `meta`
  table (`crates/txtodo-store/migrations/0001.sql` already defines `meta (key, value)`).
- Wired into the `McpBackend` mutation path: `mcp_backend.rs` calls `rate.check` before building the
  `ApplyRequest`; a `Pause` becomes an MCP error (mcp-server-tools' `McpError`) and skips `Apply`.
- The notification event rides the existing `Watch`/event channel (`daemon` watch_task), no new bus.

## Edge cases & invariants

- **Sliding window, not per-minute buckets.** A burst of 60 at second 59 and 1 at second 61 pauses
  the token even though they straddle a clock minute.
- Deletes and mutations count independently: 60 adds + 10 deletes is fine; the 11th delete pauses.
- Distinct tokens have independent budgets — one token's pause never throttles another.
- The clock is injected; tests drive fake time to cross the 60 s boundary with no sleep.
- The bucket map is bounded by `MAX_TRACKED_TOKENS` (asserted); idle tokens are evicted rather than
  letting the map grow without limit.
- Pause is persisted in `meta`, so a daemon restart does not reset a paused token to active.
- The event carries `token_id` + `name` + `reason`, never token text (mcp-auth discipline).

## Acceptance

- The 61st mutation within a minute returns `Pause` and emits `TokenPaused`; the 11th delete does
  the same.
- A paused token's subsequent mutation is rejected (MCP error), and the pause survives a daemon
  restart (read back from `meta`).
- Two tokens: one paused at 61 mutations, the other still completes 60 — independent budgets.
- The bucket map never exceeds `MAX_TRACKED_TOKENS` (assert + test hammering many token ids).

## References

- plan M6 (txtodo-implementation-plan.md), design §6.5 (txtodo-design.md)
- store `meta` table: `crates/txtodo-store/migrations/0001.sql`
- fakes/clock idiom: `.claude/stack.md` (inject the clock, seeded PRNG, no sleeps)
- tokens: [../mcp-tokens/notes.md](../mcp-tokens/notes.md) · errors: [../mcp-server-tools/notes.md](../mcp-server-tools/notes.md)
