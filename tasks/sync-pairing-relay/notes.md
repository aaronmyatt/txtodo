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

### Open design question (resolve first, before writing code)
`holepunch.rs`'s group gate exists to stop an already-paired device from being tricked into dialing
a stranger over the relay. Pairing needs *some* replacement guard, since "no gate at all" would let
any relay client dial any other client's pairing endpoint. Two directions, pick one and update this
file before implementing:
- (a) A pairing-specific relay dial that gates on the pairing offer's one-time `nonce`/code instead
  of `GroupId` — the joiner must already know the nonce (from the QR/text code), so this is no
  weaker than the LAN path's guarantee.
- (b) Extend `holepunch::connect` to accept an alternate gate predicate instead of only `GroupId`,
  so both the sync path and the pairing path share one dial implementation.

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
