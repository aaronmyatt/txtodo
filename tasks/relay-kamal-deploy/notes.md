# Deploy the hosted relay + accounts service, colocated, via Kamal (ADR 0027)

Full topology and rationale: `docs/relay-deployment-plan.md`. This file records only what an
implementer needs that the plan doesn't repeat.

## Blocked on, and not by this task

Priority is low (D), set 2026-09-20. Two things stood in the way of real cross-network relay sync.

- The endpoint-id collision (2026-09-15): a real relay refused the second endpoint with *"Another
  endpoint connected with the same endpoint id."* The shared per-device relay endpoint (root todo
  18, `daemon-shared-sync-link`) closed on 2026-09-15. It is not proven on a real relay; the last
  acceptance line does that.
- An unstable relay identity (2026-09-20). `txtodod` started with no `--key-store` (how launchd and
  the desktop start it) mints a fresh relay identity on every start, so an `access.allowlist` entry
  goes stale at each restart. The fix is the root Decide on the default `--key-store`. Do not seed
  the allowlist before it lands.

Accounts is deferred (2026-09-20; see the root Decide on who needs to enrol). Only phase 1, the
static allowlist, needs deploying. The phase 2 flip below waits on that decision.

**So: the droplet can be stood up and the allowlist proven in isolation, but "two machines on
different networks sync" cannot be trusted until the relay identity is stable.** Don't let a green
deploy read as a working system.

## Why the WebSocket proof is item 1

The iroh relay protocol is a long-lived WebSocket upgrade over HTTPS. This topology puts kamal-proxy
in front of it. If kamal-proxy mangles the upgrade, or idles the connection out on a timer, the
relay is silently useless — and it will look like an iroh bug, not a proxy config. One afternoon
proving this against a trivial echo backend saves that entire misdiagnosis. Check kamal-proxy's HTTP
idle timeout explicitly rather than assuming the default is generous.

## Config facts, read from iroh-relay 1.2.0's own source

- **No `tls` block ⇒ every service is served over plain HTTP on `http_bind_addr`.** That is exactly
  the behind-a-proxy shape; it is a supported mode, not a workaround.
- **`enable_quic_addr_discovery` requires `tls` in the relay itself**, so it cannot be on while
  kamal-proxy owns 443. It defaults to `false`. Cost, restated because it is easy to forget: nodes
  won't learn their public address this way and most traffic will really traverse the droplet.
- Default ports if anything is ever run without the proxy: HTTP 80, HTTPS 443, QUIC address
  discovery UDP 7842, metrics 9090. There is no STUN port — iroh 1.x replaced STUN with QUIC address
  discovery entirely.
- `access.allowlist`, `access.denylist`, `access.shared_token` and `access.http` are all config-file
  features of the stock binary. Nothing here needs a fork.

## Deploy hygiene worth stating

- **Deploys drop live relay connections.** kamal-proxy drains on each deploy; clients reconnect.
  Expected, not a bug — but write it in the runbook before someone debugs it at 11pm.
- **Revocation path must be fast and documented.** Phase 1 it is editing the allowlist and
  restarting; phase 2 it is one `UPDATE` in the accounts DB. The runbook item exists because the
  moment you need this, you need it immediately.
- **`relay/` is not part of this deployment.** Different server, no authentication by design,
  nothing in the client talks to it. Keep it off the droplet, or on loopback, until it has its own
  access story.

## Acceptance

The last two items in `todo.txt`. The refusal case matters as much as the success case: confirm the
relay's own log shows the denial, because the client side gets no reason back
(`Access::Deny { reason: None }`), which is exactly why `cli-relay-enroll` puts enrollment state in
`txtodo doctor`.
