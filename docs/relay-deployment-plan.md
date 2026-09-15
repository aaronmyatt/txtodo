# Relay deployment plan — node-id access control, colocated, Kamal on one droplet

Decisions taken: gate on **iroh node id** via the relay's `access.http` hook; **colocate** the relay
and the accounts service on one DigitalOcean droplet, deployed with Kamal.

## 1. Topology

One droplet, one public IP, two hostnames, three containers.

| | Container | Port | Public? |
|---|---|---|---|
| kamal-proxy | (Kamal's own) | 80, 443 | yes — owns TLS/ACME for both hostnames |
| `iroh-relay` | pinned image | plain HTTP, proxied | via `relay.example.com` |
| `accounts` | built from this repo | plain HTTP, proxied | `accounts.example.com`, enrollment routes only |

- kamal-proxy terminates TLS for both names. The relay runs with **no `tls` block** — it then
  serves every service over plain HTTP on `http_bind_addr`, which is exactly the
  behind-a-proxy shape.
- `enable_quic_addr_discovery = false` (the default). It *requires* the relay to hold its own
  cert, which conflicts with kamal-proxy owning 443. **Consequence: nodes won't learn their public
  address this way, hole-punching mostly fails, and cross-network traffic really traverses the
  droplet.** At personal scale, with ops measured in bytes, that's noise. Revisit only if bandwidth
  shows up — the fix is the relay on its own droplet with its own ACME and UDP 7842 open.
- The access-check route is **not public**: the relay calls the accounts container over the Docker
  network. Only `/enroll/*` needs to be reachable from the internet.

## 2. Access control

The relay handshake authenticates the client's node id *before* the access hook runs (the client
signs a challenge / exported TLS keying material). So node id is a trustworthy subject, not a
claim. `access.http` POSTs per connection with header `X-Iroh-Endpoint-Id: <hex>`; return `200`
with body exactly `true` to admit, anything else denies.

```toml
# phase 1 — just me
access.allowlist = ["<laptop relay node id>", "<desktop relay node id>"]

# phase 2 — allowlist lives in the accounts service
access.http = { url = "http://accounts:8080/relay/access" }
# IROH_RELAY_HTTP_BEARER_TOKEN so the accounts service can tell the relay apart from anyone else
```

## 3. Phase 0 — droplet and Kamal skeleton

- Droplet (smallest tier is ample), DNS `relay.` and `accounts.` A records, firewall 22/80/443.
- `kamal setup`, secrets via `.kamal/secrets` → 1Password/env, never committed.
- **Verify first, before building anything else:** that kamal-proxy correctly proxies a
  long-lived WebSocket upgrade to a backend container, and doesn't idle it out. The relay protocol
  *is* a WebSocket; if this doesn't hold, the whole topology is wrong and it's better to know on
  day one. Check kamal-proxy's HTTP idle timeout setting explicitly.
- Also settle early: whether the relay is a Kamal **accessory** (pinned third-party image, doesn't
  redeploy with our code — conceptually right) or its own Kamal **app** (guaranteed kamal-proxy
  host routing). If accessories can't sit behind kamal-proxy cleanly, use two apps.

## 4. Phase 1 — relay up, allowlist, personal use

- Build/pin an `iroh-relay` image: `cargo install iroh-relay --features server` (crate version 1.2,
  matching the `iroh = "1"` this workspace already depends on).
- Config: `enable_relay = true`, `http_bind_addr = [::]:3340`, no `tls`, `access.allowlist = [...]`.
- Point both machines at it: `txtodod --relay https://relay.example.com`.
- **Blocked on one small code gap:** nothing prints this device's own relay node id. It exists on
  `PairOfferResponse.relay_node_id` but not on `HealthResponse`, so `txtodo doctor` can't show it,
  and you can't populate the allowlist without it. Add the field to Health plus a doctor line (or a
  `txtodo relay id` command) — a one-sitting task, and it's also the input the phase-2 enrollment
  CLI needs.
- Acceptance: a node id **not** in the list is refused; the two that are converge across networks.
  This is also the first real chance to re-run the two `#[ignore]`d relay tests — but note they're
  still blocked by the endpoint-id collision (todo 18), independent of anything here.

## 5. Phase 2 — accounts service, OTP-verified email allowlist

New workspace member `accounts/`, axum + rusqlite, mirroring `relay/`'s shape and its
zero-dependencies-on-txtodo-crates discipline (`relay/tests/no_txtodo_deps.rs` is the precedent —
this service has no business parsing anything of ours either).

Two tables:

- `accounts(email PK, verified_at, revoked_at)`
- `devices(endpoint_id PK, email FK, enrolled_at, revoked_at)`
- plus `otps(email, code_hash, expires_at, consumed_at, attempts)`

Three routes:

- `POST /relay/access` — private. One indexed lookup on `X-Iroh-Endpoint-Id`; `true` iff the device
  is enrolled, its account verified, neither revoked. Verify the relay's own bearer token.
- `POST /enroll/request {email}` — public. Allowlisted emails only at first (your seed list).
  Rate-limited per email and per IP. Mails a 6-digit code, short expiry.
- `POST /enroll/verify {email, code, endpoint_id}` — public. Single-use, constant-time compare,
  attempt-capped. On success binds that node id to the account.

Client side: `txtodo relay enroll <email>` then `txtodo relay enroll --code NNNNNN`, which reads the
local relay node id (§4's gap) and posts it. After enrollment the daemon changes nothing about how
it connects — the gate reads the node id off the handshake, so **no protocol or crypto change on
the client at all**.

Email delivery needs a real provider (Postmark/Resend/SES) with the API key in Kamal secrets.
Deliverability is the part that most often feels like a bug.

## 6. Risks worth pricing in now

- **Fail-closed.** Any error from the accounts service → `Access::Deny { reason: None }`. Accounts
  down means nobody syncs. Colocation makes this much less likely (no network hop), but add a
  short-TTL in-process cache of admitted node ids, and keep `access.allowlist` in the config as a
  break-glass fallback for your own devices.
- **Per-connection latency.** `on_connect` awaits the HTTP call. Single indexed SELECT, no N+1.
- **PII boundary.** Emails live only in the accounts DB. The relay process never sees one — only
  hex node ids. Keep it that way; it's what preserves the design claim the relay currently makes.
- **Deploys drop connections.** kamal-proxy draining kills live relay WebSockets each deploy.
  Clients reconnect; worth knowing before you read it as a bug.
- **`relay/` in this repo is a different server.** No auth by design, nothing talks to it yet. None
  of the above protects it. Keep it undeployed, or on loopback, until it has its own story.

## 7. Backlog items this produces

1. Expose this device's relay node id on `HealthResponse` + `txtodo doctor` (blocks phase 1).
2. `accounts/` crate: schema, `/relay/access`, OTP request/verify, rate limits.
3. `txtodo relay enroll` CLI.
4. Kamal config: two services, secrets, the WebSocket-proxy verification above.
5. Decide and record an ADR — it supersedes ADR 0018's "hosted SaaS, manual grants" with the
   concrete mechanism, and settles the README/`docs/relay.md` "self-host only" contradiction.
