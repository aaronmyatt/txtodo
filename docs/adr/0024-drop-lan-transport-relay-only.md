# 0024 — Drop LAN transport entirely; sync always goes through the relay

- Status: superseded by 0026 — its premise (LAN was never wired into the daemon) was false at the
  time this was written; see 0026.
- Date: 2026-09-13
- Deciders: project owner (docs/questions.md Q11)

## Context
ADR 0003 chose iroh for QUIC/hole-punching/relay and `mdns-sd` for LAN discovery, with M4 enabling
local discovery only and M8 turning on relay as a second carrier. In practice, the LAN path was
never exercised host-to-host: its only test (`crates/txtodo-sync/src/endpoint_tests.rs:74`) is
`#[ignore]`d and forces both ends onto `127.0.0.1`, and `Discovery`/`PeerTable`/`Link` were never
wired into the daemon (`crates/txtodo-daemon/src`). Q11 asked whether to finally wire and test LAN
discovery for a real two-host run, or skip it in favor of M8's relay/file-carrier path.

## Decision
We will drop LAN transport entirely rather than defer it. All sync always goes through the hosted
relay (ADR 0018) — there is no local QUIC/mDNS path, ever, regardless of device count or network
topology. This supersedes the LAN-discovery half of ADR 0003: `mdns-sd` and local-only discovery
are dropped; iroh's QUIC/relay/hole-punching role for the relay path itself is unaffected by this
decision.

## Consequences
- Good: one transport path instead of two — simpler to build, test, and reason about; the
  loopback-only test's restriction is no longer a blocker to resolve, because the path it half-
  tested doesn't ship.
- Bad: `sync-lan-transport` (todo 2) is no longer "wire this in later" but dead — `Discovery`,
  `PeerTable`, `bind_local_endpoint`, and the ignored `endpoint_tests.rs:74` test become removal
  candidates rather than future work. `sync-pairing`'s handoff (todo 3) and
  `test-nested-ref-sync`'s fresh-device half (todo 20) need re-scoping to the relay path only. Sync
  now has a hard dependency on relay reachability — two devices on the same LAN with no internet
  cannot sync at all.
- Neutral / follow-ups: `mdns-sd` becomes an unused dependency once the above is removed.

## Alternatives considered
- Wire discovery + `Link` into `txtodod` now and run a real two-host test (the original
  recommendation): resolves whether the loopback restriction hid a real cross-host bug, but builds
  and validates a transport path this decision removes anyway.
- Leave LAN QUIC unwired but keep it as a future option (defer, don't drop): keeps two transport
  paths in the design indefinitely instead of committing to one.
