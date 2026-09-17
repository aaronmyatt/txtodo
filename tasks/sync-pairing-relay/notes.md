# Pairing across networks: relay-carried handshake when LAN can't reach the peer (plan M8, ADR 0026)

## Goal

`txtodo pair` / `txtodo pair <code>` works today only when both devices are discoverable on the
same LAN (`crates/txtodo-daemon/src/pairing_lan.rs`, mDNS + iroh QUIC). ADR 0026 keeps that path as
the primary one, but cross-device dogfooding across different networks needs pairing to also
succeed when LAN discovery fails or times out — the same X25519/SAS handshake, carried over a
relay-backed connection instead.

This is a sibling to `sync-relay-enable` (which makes *ongoing sync* fall back to relay after
pairing has already happened), not a duplicate of it: pairing happens *before* two devices share a
group key, so it cannot reuse `sync-relay-enable`'s group-keyed relay routing as-is.

## Design

### What already exists and can be reused
- `crates/txtodo-sync/src/pairing_relay.rs`: the wire messages for the pairing handshake's
  daemon-to-daemon leg (`JoinerHello`, etc.) are already `Link`-generic — the module doc is explicit
  that these bytes are "carried over a `Link` on its own connection," not hardcoded to
  `lan_link::IrohLink`. This format does not need to change.
- `crates/txtodo-sync/src/holepunch.rs`'s `RelayEndpoint`/`connect`/`accept` already do relay +
  hole-punch dialing by iroh `NodeId` — but `connect()` gates on `GroupId` first
  (`HolepunchError::ForeignGroup`), refusing to dial a peer outside the caller's group. Pairing has
  no group yet, so this gate cannot apply to the pairing dial as written.

### Open design question — RESOLVED 2026-09-14: option (a)
`holepunch.rs`'s group gate exists to stop an already-paired device from being tricked into dialing
a stranger over the relay. Pairing needs *some* replacement guard, since "no gate at all" would let
any relay client dial any other client's pairing endpoint. Two directions were on the table:
- (a) A pairing-specific relay dial that gates on the pairing offer's one-time `nonce`/code instead
  of `GroupId` — the joiner must already know the nonce (from the QR/text code), so this is no
  weaker than the LAN path's guarantee.
- (b) Extend `holepunch::connect` to accept an alternate gate predicate instead of only `GroupId`,
  so both the sync path and the pairing path share one dial implementation.

**Decision: (a).** Investigating the actual LAN pairing path (`pairing_lan.rs`) before writing any
relay code found that the LAN path's *real* security boundary was never `GroupId` in the first
place: `LanEndpoint::connect_pairing` (the method the LAN joiner already dials through) has no
group check at all — it dials by node id alone, over a dedicated `PAIRING_ALPN`. The actual gate is
`pairing_lan.rs::process_hello`, which refuses any `JoinerHello` whose `nonce`/`group` do not match
this daemon's own active offer, checked *after* the connection exists but *before* any crypto state
advances. That check is already carrier-agnostic (it reads the decoded `JoinerHello`, not the
`Link` it arrived over) — a relay-carried pairing connection gets the identical protection for
free, so option (a) needs no new "gate" abstraction at all, only a new relay-specific *dial*:
- `RelayEndpoint::connect_pairing(node)` (txtodo-sync `holepunch.rs`), the relay twin of
  `LanEndpoint::connect_pairing` — dials by node id over `PAIRING_ALPN`, no `GroupId` parameter,
  no `ForeignGroup` check. Mirrors the LAN method exactly; `holepunch::connect`'s own `GroupId` gate
  (used by the already-merged, tested ongoing-sync relay path, `sync-relay-enable`/
  `relay-converge-test`) is untouched — a second method, not a modified one.
- The daemon's relay accept loop (`relay.rs`) dispatches an incoming connection by ALPN, exactly
  the way `lan.rs::accept_one` already does: `PAIRING_ALPN` routes to `pairing_lan::handle_incoming`
  (unchanged — same `JoinerHello`/`InitiatorReply` state machine, whichever carrier delivered the
  frame). No crypto or SAS-flow code changes anywhere; only a second way to reach the same handler.
- Option (b) was rejected: it would have required touching `holepunch::connect`'s existing
  group-gated logic (or at minimum its call sites) that `relay-converge-test`'s already-merged,
  tested ongoing-sync path depends on — real risk for no benefit, since (a) reuses the existing
  nonce gate for free and touches zero pre-existing sync-path code.

**Known limitation, flagged, not fixed this pass**: `RelayEndpoint::connect_pairing` dials through
`self.relay_url` — the *dialing* device's own configured relay, not necessarily the initiator's
(carried separately in the offer's `relay_url` field for a human/future multi-relay routing layer
to use). M8's own scope is one relay (design §4.5); this only actually rendezvous when both devices
share the same relay URL, the same limitation `relay_fallback_dial`'s doc already names for
ongoing sync. Not re-solved here — a multi-relay directory is out of scope.

### Wire format extension
`PairingCode`/`PairOfferResponse` (`crates/txtodo-cli/src/commands/pair.rs`,
`crates/txtodo-daemon/src/pairing_wire.rs`) currently carry only a LAN `endpoint`. Add an optional
relay rendezvous field (the offering device's iroh `NodeId`, plus the relay URL it's configured
with) so a joiner who can't reach the LAN `endpoint` within a short bounded timeout falls back to
dialing the relay path with the same nonce/SAS flow. Presence of a relay field must never be
required — an offer made with no relay configured simply omits it and LAN-only pairing behaves
exactly as it does today.

### Daemon wiring
`crates/txtodo-daemon/src/pairing_lan.rs`'s background relay-driver task (confusingly named — see
`pairing_relay.rs`'s own doc comment: this is the *daemon-to-daemon pairing carrier*, unrelated to
the M8 ciphertext-blob relay server) needs a sibling that races LAN discovery against a relay dial
(once the design question above is resolved) and uses whichever connects first, same idea as
`sync-relay-enable`'s carrier-order plan (LAN, then direct, then relay) but for the pairing leg
specifically.

## Acceptance
- Two daemons with no shared network path (simulated the same way `relay-converge-test` does it —
  separate network namespaces) can still complete `txtodo pair` / `pair <code>` end to end, given a
  reachable relay, with the same SAS-confirmation UX as the LAN path.
- An offer made with no relay configured pairs over LAN exactly as it does today (no regression).
- The relay path never lets a device join a group without a matching nonce/code — a relay client
  guessing at random cannot pair itself in.
- `txtodo doctor` reports which carrier a completed pairing actually used (LAN vs relay).

## Bug fix: swallowed pairing-dial errors (2026-09-16)

Doing an actual two-real-device pairing test by hand: `pairing_lan.rs::attempt` (LAN dial) and
`pairing_relay_dial.rs::relay_attempt` (relay dial) swallowed every connect/send/recv failure with
`.ok()?`, so a failed pairing round left literally nothing in the daemon log even at debug level —
making a real cross-network pairing failure undiagnosable from the outside.

Fixed: both now log a named `debug!()` line per failure point
(`pairing_joiner_lan_connect_failed`, `pairing_joiner_relay_connect_failed`,
`pairing_joiner_relay_no_rendezvous_in_offer`, `pairing_joiner_relay_endpoint_not_bound`,
`pairing_joiner_lan_round_no_reply`, `pairing_joiner_relay_round_no_reply`). No behavior change.

## As built (2026-09-14)

Design decision (a): a nonce-gated `RelayEndpoint::connect_pairing` (`txtodo-sync`), no `GroupId`
gate needed since `process_hello`'s existing nonce/group check is already carrier-agnostic.
`pairing_grpc.rs` attaches the offering device's relay rendezvous to the offer when bound;
`pairing_relay_dial.rs` (daemon) races LAN vs relay per joiner round; `relay.rs` dispatches
`PAIRING_ALPN` connections to the same handler LAN uses; `txtodo doctor` reports which carrier a
completed pairing used. Two real `txtodod` processes pair end to end over a real public relay with
`--no-lan` on both (`tests/pairing_relay.rs`); a wrong-nonce relay dial is rejected and doesn't
disturb the real offer; `tests/pairing_lan.rs` and `tests/pairing.rs` (cli) re-verified green, no
regression.

Found and fixed a real bug along the way: `run_joiner` previously bailed out entirely with no LAN
endpoint at all, which would have made the relay path unreachable under `--no-lan` regardless of
what the offer carried.

**Doc-drift found and fixed 2026-09-16** (during `daemon-workspace-session-multiplex` stage 2,
while answering "can two devices pair with no shared LAN today"): `relay.rs`'s own module doc and
`txtodo-daemon/CLAUDE.md` still called real pairing-over-relay "sync-pairing-relay's own
not-yet-built task" — stale since this landed. Both corrected to say it's done and name
`pairing_grpc.rs`/`pairing_relay_dial.rs`.
