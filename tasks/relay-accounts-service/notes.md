# accounts/: OTP-verified email allowlist keyed by iroh node id (ADR 0027 phase 2)

## Goal

The service that answers the hosted iroh relay's access check. It owns emails, OTP verification, and
the node-id→account binding. The relay itself never learns an email — it POSTs a node id and reads
`true` or not.

## The contract, exactly

The stock `iroh-relay` binary's `access.http` mode (verified in `iroh-relay` 1.2.0's `src/main.rs`,
`HttpAccess`/`http_access_check_inner`):

- Method `POST` to the configured URL, once per incoming relay connection.
- Header `X-Iroh-Endpoint-Id: <hex endpoint id>`.
- Optional `Authorization: Bearer <token>` if the relay is configured with one
  (`IROH_RELAY_HTTP_BEARER_TOKEN`).
- **Admit iff the response is `200` with body text exactly `true`.** Any other status, any other
  body, and any transport error are all `Access::Deny`.

That last line is the design constraint worth internalising: this service failing means nobody
syncs. Colocation (`relay-kamal-deploy`) removes the network hop; the in-process cache and keeping
`access.allowlist` configured as break-glass are the other two mitigations.

## Why node id is a safe subject

`iroh-relay`'s `ClientRequest::endpoint_id()` is documented as authenticated *before* the access
hook runs — "the client proves possession of the secret key for this public key by either signing
keying material exported from the TLS session or a challenge issued by the server". So this service
is authorising an established identity, not a self-asserted one. That is the whole reason ADR 0027
chose node id over `access.shared_token`.

## Shape, and what it borrows

Deliberately modelled on `relay/`, which is the closest thing in this repo and the right precedent:

- **Zero `txtodo-*` dependencies, asserted by a test that reads the manifest** — `relay/tests/
  no_txtodo_deps.rs` is the pattern. This service has no business parsing a todo line either, and
  the structural guarantee is worth more than the convention.
- **A `Mailer` trait with a logging no-op first**, exactly how `relay::push::Push`/`NoopPush` seams
  the APNs/FCM client it doesn't have yet. Lets every test run without a provider.
- **Named, checked caps for every collection and every rate** — the repo convention, and here they
  are also the abuse controls.
- axum + rusqlite are already pinned in this workspace at the versions `relay/` uses, so no new
  licence review in `deny.toml`.

## Trust boundary

- `/relay/access` is **private**: reachable only from the relay container over the Docker network,
  and additionally gated on the relay's bearer token. Without that second gate, anyone who can reach
  the service can enumerate which node ids are enrolled.
- `/enroll/*` is **public** — the only routes that are.
- Emails live here and nowhere else. The relay process, its logs, and its database stay free of
  them, which is what keeps design §4.6's claim about the relay honest.

## Open, decide before building

- **Email provider**: Postmark / Resend / SES. Deliverability is the part that most often reads as a
  bug, so pick one with a reputation and put the API key in Kamal secrets.
- **Seed allowlist**: which emails may even request a code during the restricted phase. Simplest is
  a config list, re-read on restart; a table is easy to add later.
- **Device limit per account**: `MAX_DEVICES_PER_ACCOUNT` needs a number. Consider how a lost laptop
  gets revoked — today that's a manual `UPDATE`, which is fine at this scale but should be stated.

## Acceptance

The four tests in `todo.txt`. The one that matters most is the third: a caller without the relay's
bearer token must not be able to learn whether a node id is enrolled.
