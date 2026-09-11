# Tests: daemons on separate network namespaces converge via relay ≤ 30 s; file-carrier converges (plan M8)

Plan M8 acceptance: two daemons on separate networks (netns in CI, or a two-VM job) converge via
relay within 30 s, via direct hole-punch when possible; file-carrier: two daemons sharing a
directory, no network, converge after each writes its ops file. Design §4.5 carriers.

## The topology is the point

M4's [sync-loopback-converge](../sync-loopback-converge/notes.md) proves two daemons on one
loopback converge. This proves the **network boundary**: NAT in between, no LAN multicast, so the
only path is hole-punch or relay. Namespaces (or two VMs) make the test honest — a relay test where
both daemons share a NIC is loopback with extra steps.

## Relay ≤ 30 s, deterministically

- CI job: create two netns, each runs `txtodod`, route only through the relay node. Assert
  convergence (identical projections) within 30 s.
- Poll convergence with a deadline, not a nap: a fast machine finishes in seconds, a slow one is
  still bounded; the timeout fails CI, no sleeps.
- Exercise hole-punch when it succeeds, and a forced-relay mode that still converges when punch is
  blocked.

## File carrier, no network

Two daemons sharing one temp dir, networking disabled: each appends to its own
`sync/<device-id>.ops`; assert convergence after both write, bounded. Same convergence assertion as
the relay test.

## The relay learns nothing (acceptance of §4.6)

Assert on the relay's store after the run: only opaque blobs; no op type distinguishable from the
stored bytes and routing metadata.

## Where it lives

CI-level integration target + a GitHub Actions job; the netns setup is a checked-in script, not a
one-off.
