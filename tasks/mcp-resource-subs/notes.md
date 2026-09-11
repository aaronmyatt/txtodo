# Resource subscriptions wired to the daemon Watch stream (plan M6, plan §6)

Plan M6: "Resource subscriptions wired to the daemon's `Watch`." Design §6.4 lists six resources
and states the daemon pushes `notifications/resources/updated` on change.

Resources (design §6.4):

```
todotxt://todo.txt        todotxt://done.txt
todotxt://task/{id}
todotxt://project/{name}  todotxt://context/{name}
todotxt://history?since=…
```

## One Watch stream, N sessions

The MCP server holds one gRPC `Watch` handle (proto `Watch`, shipped M3/M5) shared by all sessions.
`resources/subscribe` adds the URI to that session's set; `resources/unsubscribe` removes it. On a
watch event, map the changed file → the resource URIs that read it, and emit
`notifications/resources/updated` with only the affected URI(s) — never a blanket "changed".

Scope matters here: a token attenuated to `project:+work` must never receive an update for a
`todotxt://project/+other` resource, even if it subscribed before attenuation. Filter the
notification against the token's scope at emit time.

## Bounds and the non-live resource

- `MAX_SUBSCRIPTIONS` per session, asserted — the subscription set is a bounded collection.
- The daemon watcher already debounces at 150 ms; coalesce bursts into one notification per URI and
  assert a bounded pending-notification queue (no unbounded buffer).
- `todotxt://history?since=…` is a computed read of the op log, not a live file — subscribing to it
  is rejected with a structured error rather than a silent no-op.

MCP resources/notification spec: <https://modelcontextprotocol.io/specification/2025-06-18/server/resources>.
