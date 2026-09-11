# MCP rate limits (plan M6, plan §6)

Goal. Per-token limits — 60 mutations/min and 10 deletes/min — with pause + notification
on exceed (design §6.5).

## Limits
- Two buckets per token: mutations (all `Apply` ops) and deletes. Sliding window, 60 s.
- Exceed either → the token is paused and a notification event is emitted.

## State
- Bounded map keyed by `token_id` — `MAX_TRACKED_TOKENS`, asserted (§3: no unbounded
  collection). Idle tokens are evicted when the map is full.
- Paused tokens reject mutations until unpaused (human) or a TTL. The pause mark lives in
  the store (meta) so a daemon restart keeps it.

## Event
- The notification event rides the daemon's event channel; the "claude-code added N tasks"
  mobile push is M9, M6 only emits the event (design §6.5).

## Acceptance
- The 61st mutation / 11th delete in a minute pauses the token and emits the event.
- Distinct tokens have independent budgets; a paused token stays paused across restart.
