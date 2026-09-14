# 0026 — Reinstate LAN transport; relay is an additive fallback, not a replacement

- Status: accepted
- Date: 2026-09-14
- Deciders: project owner

## Context
ADR 0024 dropped LAN transport entirely on the premise that `Discovery`/`PeerTable`/`Link` were
never wired into the daemon (`crates/txtodo-daemon/src`) and that only an `#[ignore]`d,
loopback-only test exercised the path.

That premise is false as of this decision. `crates/txtodo-daemon/src/{lan.rs,lan_session.rs,
pairing_lan.rs,lan_op_signing.rs,lan_status.rs}` wire LAN discovery, the QUIC `Link`, and pairing
into the real daemon. `crates/txtodo-daemon/tests/pairing_lan.rs`'s
`two_real_daemons_pair_for_real_and_the_joiner_receives_the_initiators_file` passes today, against
two real daemon instances, a real X25519/SAS handshake, and real group-key delivery — not a mock,
not `DebugSetGroupKey`. Commit `b099180` (real cross-device pairing over LAN) predates ADR 0024's
own commit; the decision was made without checking the code that already existed.

## Decision
LAN transport is reinstated as the primary sync path. Relay (ADR 0018's hosted relay, ADR 0003's
iroh relay/hole-punch role) is an *additive* fallback carrier, used only when two devices can't
reach each other directly — different networks, one device behind an unfriendly NAT, etc. Two
devices on the same LAN with no internet must still be able to sync; that was ADR 0024's
explicitly accepted regression, and it is rejected here.

This re-supersedes the LAN-discovery half of ADR 0024, and restores ADR 0003 (`mdns-sd` + iroh
QUIC for LAN) to standing alongside iroh's relay role. `mdns-sd` is not an unused dependency.

## Consequences
- Good: same-LAN sync keeps working offline and fast (sub-2ms convergence, already measured);
  cross-network sync becomes possible via relay without taking anything away.
- Good: no working, tested code is deleted to satisfy a premise that didn't hold.
- Bad: two transport paths instead of one — more surface to build, test, and reason about, which
  is exactly the complexity ADR 0024 was trying to avoid. Accepted: the alternative was deleting
  real functionality.
- Follow-up work (scoped separately, `tasks/sync-relay-enable/`, `tasks/relay-converge-test/`,
  and a new pairing-over-relay task): the daemon must actually construct and use a relay/hole-punch
  carrier when LAN can't reach a peer — today `crates/txtodo-daemon/src/lan_status.rs`'s
  `RELAY_DISABLED: bool = true` means relay is built as a library (`crates/txtodo-sync/src/relay.rs`,
  `holepunch.rs`) and a standalone server (`relay/`) but is not wired into the daemon's sync or
  pairing flow. Pairing itself is LAN-discovery-only today (`txtodo pair`/`pair <code>` via mDNS);
  cross-network dogfooding needs a relay-reachable pairing path too, which nothing currently
  provides — `crates/txtodo-sync/src/pairing_relay.rs` is the LAN pairing handshake's wire format,
  not a relay-carried pairing path, despite the name.

## Alternatives considered
- Leave ADR 0024 as-is and build relay-only pairing/sync from scratch: rejected — throws away
  tested, working code to satisfy a decision whose stated justification doesn't match the codebase.
- Keep two full independent stacks with no shared fallback logic: rejected — the point of treating
  relay as *additive* is one `Link`-driven session with a carrier-selection order (LAN, direct,
  relay), not a second parallel implementation.
