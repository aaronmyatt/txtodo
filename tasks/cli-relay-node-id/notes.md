# Expose this device's own relay node id (ADR 0027 prerequisite)

## Goal

A human must be able to read this device's relay node id without attaching a debugger or starting a
pairing handshake. Every access-control item in ADR 0027 needs it: phase 1's
`access.allowlist = [...]` is literally a list of these, and phase 2's `txtodo relay enroll` posts
one.

## Why it isn't already possible

The id exists on the wire exactly once, in `PairOfferResponse.relay_node_id`
(`crates/txtodo-proto/proto/txtodo/v1/txtodo.proto:450`), and only when a relay is configured *and*
bound at offer time. `HealthResponse` — the message `txtodo doctor` renders — carries `relay_url`
and `relay_last_outcome` but not the id. So the only way to see it today is to run `txtodo pair` and
read a field out of the JSON offer, which starts a real handshake as a side effect.

## What to reuse

- `crates/txtodo-daemon/src/pairing_grpc.rs:50` — `relay_rendezvous(&ws)` already returns
  `Option<(node_id_hex, url)>`. Feed both `PairOffer` and `Health` from it rather than writing a
  second accessor; two sources for one fact is how they drift.
- `crates/txtodo-daemon/src/health_grpc.rs:35` — `relay_url`/`relay_last_outcome` are already
  populated here; the new fields sit beside them.
- `crates/txtodo-cli/src/commands/doctor_transport.rs` — `transport_check` already composes its
  message from `relay_summary` + `paired_summary`. The node id belongs in `relay_summary`, not as a
  new check: a missing relay is not a failure (ADR 0026 — relay is additive).

## Design notes

- **Hex, not bytes.** `PairOfferResponse` already carries it hex-encoded; matching that means the
  string a human copies into a relay config is the same string the pairing path emits.
- **Stability is the whole point, and it already holds.** The per-device relay identity has been
  persisted since `daemon-workspace-identity-agreement` stage 1 (`device_identity.rs`), so the id
  survives restarts. The test below is really a regression guard on that property — if someone
  reintroduces a per-run mint, every deployed allowlist silently breaks.
- **Say nothing when there is no relay.** Empty string, and doctor's line unchanged. A device with
  no relay configured is a normal, supported state.

## Acceptance

Both tests in `todo.txt`, plus: `txtodo doctor` on a relay-configured daemon prints a 64-hex-char id
a human can paste straight into `access.allowlist`.

## Not in scope

`txtodo relay enroll` (its own task, `cli-relay-enroll`) and anything server-side. This task only
makes an existing fact legible.

## As built (2026-09-20)

- proto `relay_node_id`/`relay_bound` on `HealthResponse` (4b4a14b, 13a9dcf); daemon fills them from
  `pairing_grpc::relay_rendezvous`, the helper `PairOffer` already used (5425984); `txtodo doctor`
  prints `node id <hex>` on the transport line once the endpoint is bound (19802c3).
- `tests/relay_node_id.rs` (177ad79): stable 64-hex id across a restart, none with no relay.

## Known gap

- The id is stable across a restart only when the daemon runs with `--key-store file|os|auto`.
  With no flag (how launchd and the desktop start it) the keystore is an in-memory placeholder and
  the relay identity is minted fresh every start, so an allowlist entry goes stale each time. The
  test runs with `--key-store file` for that reason. Fixing it is a keystore-default decision, filed
  as its own `@human` line in the root `todo.txt`.
