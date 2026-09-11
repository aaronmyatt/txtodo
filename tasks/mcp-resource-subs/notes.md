# Resource subscriptions wired to the daemon Watch stream (plan M6, plan §6)

Plan M6: "Resource subscriptions wired to the daemon's `Watch`." Design §6.4 defines the six
subscribable resources and states the daemon pushes `notifications/resources/updated` on change.
The wire side already exists: proto `rpc Watch(WatchRequest) returns (stream Change)` with
`WatchRequest { repeated string paths }` and `Change { path, hash, ops }`
(`crates/txtodo-proto/proto/txtodo/v1/txtodo.proto`). This task is the mapping layer between that
stream and MCP resource URIs.

## Design

Resources (design §6.4), with what each reads:

| URI | source | live? |
|---|---|---|
| `todotxt://todo.txt`, `todotxt://done.txt` | file bytes | yes |
| `todotxt://task/{id}` | one task from the projection | yes |
| `todotxt://project/{name}`, `todotxt://context/{name}` | filtered view (query §8) | yes |
| `todotxt://history?since=…` | op-log page | no — computed read |

One `Watch` handle, N sessions. The daemon already offers
`handle.subscribe() -> Result<broadcast::Receiver<Change>, ActorError>` per file actor
(`crates/txtodo-daemon/src/handle.rs`). The MCP server holds one shared gRPC `Watch` stream and
fans a change out to the sessions whose subscription set covers a URI that reads the changed file.

```rust
// crates/txtodo-daemon/src/mcp/watch.rs
pub struct SubscriptionSet { uris: VecSet<Uri>, max: usize }   // bounded by MAX_SUBSCRIPTIONS

/// Changed file path -> the resource URIs that read it (pure, exhaustive).
pub fn uris_for(path: &FilePath) -> Vec<Uri>;
// todo.txt -> [todotxt://todo.txt, todotxt://project/{p} for p in file, todotxt://context/{c}…]

/// Emit per affected URI, filtered against the session's scope.
pub async fn dispatch(change: Change, sessions: &[Session]) -> Vec<Notification>;
```

`resources/subscribe` adds the URI to that session's set; `resources/unsubscribe` removes it. On a
`Change`, run `uris_for(change.path)`, intersect with each session's set, and emit
`notifications/resources/updated` carrying only the affected URI(s) — never a blanket "changed".

## Scope filtering happens at emit time, not subscribe time

A token attenuated to `project:+work` must never receive an update for `todotxt://project/+other`,
even if it subscribed before attenuation. Filter against the token's resolved `Scope` at emit time:
a `+other` URI update is dropped for a `+work` session. This is the same restrictor-intersection
rule the [mcp-scope-matrix-test](../mcp-scope-matrix-test/notes.md) pins.

## Placement/dependencies

- `txtodo-mcp` declares the resource schemas and the `resources/subscribe` + `resources/unsubscribe`
  handlers; `allowedDeps` = `txtodo-proto`, `txtodo-query`.
- `txtodo-daemon` owns the `Watch` handle and the fan-out because only it reaches the actors.
- Query filtering for `project/{name}` / `context/{name}` reuses `txtodo-query` (already an
  `txtodo-mcp` dependency) — do not re-implement §8 in the MCP crate.

## Edge cases & invariants

- `MAX_SUBSCRIPTIONS` per session, asserted — the subscription set is a bounded collection
  (constitution §3: queues, caches, collections carry an explicit max).
- The daemon watcher debounces at `DEBOUNCE_MS = 150` (`crates/txtodo-daemon/src/debounce.rs`).
  Coalesce bursts into one notification per URI and assert a bounded pending-notification queue —
  no unbounded buffer, a full queue drops with a warning like the watcher does (`watcher.rs`).
- `todotxt://history?since=…` is a computed read of the op log, not a live file — subscribing to it
  is rejected with the structured error from [mcp-batch-dry-run](../mcp-batch-dry-run/notes.md),
  not a silent no-op.
- A subscription to a URI that matches nothing (e.g. an empty `+project`) is allowed but emits
  nothing; assert it does not error.
- Sessions disconnect: dropping a session removes its subscription set, so the fan-out never
  delivers to a dead receiver.

## Acceptance

- An external edit to `todo.txt` reaches a subscribed session as exactly the
  `notifications/resources/updated` for the `todotxt://todo.txt` URI within the debounce window.
- A session subscribed to `todotxt://project/+work` does not receive a `+other` update.
- Subscribing to `todotxt://history?since=…` returns a structured error.
- Unsubscribe stops the notifications; a second subscription to the same URI is idempotent.

## References

- MCP resources + subscriptions: https://modelcontextprotocol.io/specification/2025-06-18/server/resources
- MCP notifications: https://modelcontextprotocol.io/specification/2025-06-18/basic/messages
- rmcp subscribe/notification plumbing: https://docs.rs/rmcp
