# Relay & file-carrier convergence tests (plan M8, design §4.5)

## Goal

Plan M8 acceptance, made deterministic in CI: two daemons on *separate networks* converge via the
relay within 30 s (design §10 SLO: "convergence within 2 s on LAN and 30 s via relay for 99.9% of
ops"), via direct hole-punch when possible; and two daemons sharing a directory with *no network*
converge via the file carrier after each writes its `sync/<device-id>.ops` file.

## Design

### Topology — the network boundary is the point

M4's loopback convergence ([sync-loopback-converge](../sync-loopback-converge/notes.md)) proves two
daemons on one host converge. This proves the boundary: a relay test where both daemons share a NIC
is loopback with extra steps. Each daemon runs in its own Linux network namespace, reachable only
through the relay node — no shared NIC, no mDNS multicast crossing the boundary.

```text
 netns A ──┐                ┌── netns B
 (txtodod) │  veth pair to  │ (txtodod)
           ├── relay netns ──┤   (reference relay binary)
           │   (blob store) │
           └── no path between A and B except through the relay
```

- CI job (GitHub Actions `ubuntu-latest`) creates two netns via a checked-in script, each running
  `txtodod` against its own temp workspace; the relay listens in a third netns.
- Convergence assertion: poll both projections with a deadline, never a `sleep` — the deadline
  fails CI if not converged, so fast machines finish early and slow machines stay bounded at 30 s.

### Relay ≤ 30 s, plus hole-punch

- **Hole-punch path:** when the two daemons can punch (relay-assisted rendezvous, design §4.5
  "Direct internet"), assert convergence happened via direct QUIC, not relay forwarding.
- **Forced-relay mode:** block hole-punch (relay in forward-only mode) and assert the 30 s bound
  still holds — the relay path is exercised, not merely present.

### File carrier — no network

Two daemons sharing one temp dir, networking disabled: each appends ops only to its own
`sync/<device-id>.ops` (design §4.5: "dumb file sync never conflicts; devices ingest each other's
files"). Assert convergence after both write, bounded by the same 30 s deadline.

### The relay learns nothing (design §4.6 / plan §5)

After the relay run, open the relay's blob store and assert: only opaque `Vec<u8>` blobs keyed by
group + device; no op type distinguishable from the stored bytes and routing metadata. This closes
the M4-deferred "relay cannot distinguish op types" checklist item.

## Placement/dependencies

- Tests: `crates/txtodo-daemon/tests/relay_converge.rs` and `…/file_carrier_converge.rs` (real
  daemons on temp dirs, same harness as M3/M4 acceptance), plus the netns setup script checked in
  beside them (`tests/support/netns.sh`).
- The GitHub Actions job wiring is a **frozen path** (`.github/**`) — ask, never silent. Land the
  test target and script first, then wire the workflow as a separate approved step.
- Depends on `relay-reference` (binary + blob layout) and
  `sync-protocol-frames`/`sync-crypto-envelope` (the encrypted, versioned, authenticated frames the
  carriers move).

## Edge cases & invariants

- No shared NIC: the netns script asserts A and B cannot reach each other except through the relay
  (a socket probe that must fail) — otherwise the test is not proving the boundary.
- Deadline, not nap: convergence is polled at a fixed interval (e.g. 250 ms) until the 30 s
  deadline; a `tokio::time::sleep(30)` would make CI slow and mask early convergence.
- File-carrier ingest must not read its own file as foreign: each daemon skips its own
  `<device-id>.ops` (identity from §4.6) or the test deadlocks on a self-loop.

## Acceptance

- Relay: two netns daemons converge (byte-identical projections) within 30 s; CI fails otherwise.
- Hole-punch is exercised when it succeeds; forced-relay still converges when punch is blocked.
- File carrier: two daemons, no network, converge after each writes its own ops file, within the
  same deadline.
- The relay's store after the run holds only opaque blobs — no op type distinguishable (assertion,
  not prose).

## References

- plan M8 (txtodo-implementation-plan.md), design §4.5/§4.6/§10 (txtodo-design.md)
- Sibling: [relay-reference](../relay-reference/notes.md), [sync-loopback-converge](../sync-loopback-converge/notes.md), [security-m8-review](../security-m8-review/notes.md)
- network namespaces: https://man7.org/linux/man-pages/man8/ip-netns.8.html
