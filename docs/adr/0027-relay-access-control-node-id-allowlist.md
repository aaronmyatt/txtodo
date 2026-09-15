# 0027 — Relay access control: node-id allowlist, OTP-verified emails, colocated accounts service

- Status: accepted
- Date: 2026-09-15
- Deciders: project owner

## Context
ADR 0018 decided the relay is a hosted SaaS with manually granted access, but named no mechanism —
and its stated justification ("the relay is now the only sync path") was voided the next day by ADR
0026, which reinstated LAN and made relay an additive fallback. Meanwhile `README.md` and
`docs/relay.md` still tell readers "this project does not run a public relay".

Both of those documents also use "the relay" for two different servers, which is the deeper
problem:

- **The iroh relay** — what `--relay <url>` actually points at (`RelayConfig.url` is parsed as an
  `iroh::RelayUrl`, `crates/txtodo-sync/src/relay.rs`). It carries QUIC traffic and coordinates
  hole-punching. It is n0's `iroh-relay` server, not a crate in this repo.
- **`relay/`** — this repo's blob mailbox: HTTP put/get/list of ciphertext, design §4.5. Built and
  tested, but no client in this workspace talks to it yet.

Only the first is on the path of any sync that happens today, so it is the one that needs a gate.

## Decision
We will run a public iroh relay, restricted first to the project owner's own devices and later to an
allowlist of OTP-verified emails, using the stock `iroh-relay` binary's own access-control hook. No
fork.

- **Gate on node id, not on a bearer token.** The relay handshake authenticates the client's
  endpoint id (the client signs a challenge or exported TLS keying material) *before* the access
  hook runs, so a node id is a cryptographically established subject rather than a claim. A shared
  token, the alternative the same binary offers, is a bearer secret that leaks and cannot be tied to
  a device.
- **Phase 1:** `access.allowlist = [<node ids>]` in the relay's config. No extra service.
- **Phase 2:** `access.http = { url, bearer_token }`, pointing at an `accounts/` service that owns
  emails, OTP verification, and the node-id→account binding. The relay POSTs per connection with
  header `X-Iroh-Endpoint-Id`; `200` with body `true` admits.
- **Colocate** the relay and the accounts service on one DigitalOcean droplet, deployed with Kamal.
  kamal-proxy terminates TLS for both hostnames; the relay runs with no `tls` block (plain HTTP
  behind the proxy) and the access-check route is reachable only over the Docker network.
- **Emails never enter the relay process.** They live in the accounts service's own database; the
  relay sees hex node ids only.

## Consequences
- Good: revocation is a database update; a stolen relay URL grants nothing; the relay stays free of
  user identity, which is what keeps design §4.6's claim about it true.
- Good: no client protocol, crypto, or wire change — the gate reads an id the handshake already
  establishes. The only client work is surfacing the local node id and an enrollment command.
- Bad: `enable_quic_addr_discovery` must stay off, because it needs the relay to hold its own
  certificate and that conflicts with kamal-proxy owning 443. Endpoints will not learn their public
  address that way, hole-punching will mostly fail, and cross-network traffic will really traverse
  the droplet. Accepted at personal scale; the fix, if bandwidth ever shows up, is the relay on its
  own host with its own ACME and UDP 7842 open.
- Bad: access checks fail closed — any error from the accounts service denies every connection.
  Colocation removes the network hop; a short-TTL cache and keeping `access.allowlist` configured as
  break-glass are the mitigations.
- Neutral: this supersedes ADR 0018's *justification* and supplies its missing mechanism; 0018's
  actual decision (hosted, manually granted) stands. `relay/`'s own no-authentication design is
  untouched and unprotected by any of this — it stays undeployed, or on loopback, until it has its
  own story.

## Alternatives considered
- `access.shared_token`: simpler, but bearer secrets leak and can't identify a device. Rejected.
- Relay owning 443 with its own built-in ACME, keeping QUIC address discovery: keeps hole-punching,
  but then kamal-proxy can't own 443 and the public enrollment routes need a second TLS story on the
  same box. Rejected for now as more moving parts than the bandwidth is worth.
- Separate hosts for relay and accounts: cleaner identity separation, but doubles the infrastructure
  for a deployment whose first user is one person. Revisit if the user count justifies it.
