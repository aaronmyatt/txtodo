# Enable iroh relay and hole-punching (plan M8, design §4.5)

## Goal

Flip the M4 decision: [sync-lan-transport](../sync-lan-transport/notes.md) built the iroh QUIC
endpoint with relay **explicitly disabled**; design §4.5 ("Direct internet" / "Relay" rows) and plan
M8 turn it on. Hole-punching is iroh's default direct path; the relay is rendezvous + fallback when
the punch fails or a peer is asleep. Refs: https://www.iroh.computer/docs · https://docs.rs/iroh.

## Design

The sync engine speaks one protocol (`txtodo-sync`) over any carrier; the transport sits behind the
`trait Link` from [sync-lan-transport](../sync-lan-transport/notes.md) — the session machine never
sees a socket. Relay/hole-punch is another `Link` impl, iroh in one module, nowhere else.

```rust
// crates/txtodo-sync/src/relay.rs — the ONE endpoint constructor, nothing bypasses it
pub struct RelayConfig { pub url: Option<String>, pub max_peers: usize }   // url None = relay off
pub fn build_endpoint(cfg: &RelayConfig) -> Result<Endpoint, LinkError>;
// relay-on  => configured relay set is exactly {cfg.url}, asserted non-empty
// relay-off => relay set empty (M4 behaviour; guards an iroh upgrade changing defaults)

// crates/txtodo-sync/src/holepunch.rs — direct + relay fallback, also a Link
pub struct RelayLink { /* iroh Endpoint + connection handles */ }
impl Link for RelayLink {
    fn send(&mut self, frame: Frame) -> Result<(), LinkError>;
    fn recv(&mut self) -> Result<Frame, LinkError>;
}
```

- `Frame`/`Hello`/`Want`/`Ops`/`Ack` come from
  [sync-protocol-frames](../sync-protocol-frames/notes.md): `Hello { device, group,
  heads: Map<DeviceId, u64>, protocol }` — the group id gates rendezvous: a peer in another group is
  dropped *before* connecting (same guard as LAN).
- Relay stays **optional** (design §4.5 "All carriers are optional"). Config is a relay URL from
  `config.toml` / `--relay`; empty ⇒ relay off, M4 behaviour unchanged.
- The relay only ever sees ciphertext — ops are already encrypted (XChaCha20-Poly1305) + signed
  (Ed25519) by [sync-crypto-envelope](../sync-crypto-envelope/notes.md). This task wires the
  transport, not crypto (design §4.6: the relay is untrusted).

## Placement/dependencies

- `crates/txtodo-sync/src/relay.rs` + `src/holepunch.rs`; config plumbing in the daemon's config +
  `txtodo` CLI (`--relay`). No new deps — iroh already landed in
  [sync-lan-transport](../sync-lan-transport/notes.md).
- Bounded collections: `MAX_RELAY_PEERS` (a relay endpoint is a peer table, unbounded unless
  capped), named `RENDEZVOUS_TIMEOUT_MS` / `CONNECT_TIMEOUT_MS`, bounded backoff with an explicit
  iteration cap (same as LAN's `MAX_BACKOFF_MS`).

## Edge cases & invariants

- One constructor, mode explicit: relay-on ⇒ configured set non-empty, relay-off ⇒ empty. The test
  catches an iroh upgrade that changes defaults (the single most useful test — see LAN notes).
- Network input is external: validate, never assert. `LinkError` variants for relay + hole-punch
  name the peer and what was attempted.
- `txtodo doctor` gains the configured relay URL and the last punch/relay outcome (design §5
  doctor: relay reachability).

## Acceptance

- Two daemons on separate networks converge via relay within 30 s; via direct hole-punch when
  possible (plan M8).
- Relay-on endpoint has a non-empty configured relay set; relay-off has an empty one.
- Two in-process endpoints rendezvous through a local iroh relay and exchange a `Hello` within a
  bounded time (callback-driven, timeout fails CI, no sleeps).
- A peer in another group is never connected to.

## References

- design §4.5 (transports table), §4.6 (trust); plan M8.
- https://docs.rs/iroh · https://www.iroh.computer/docs/layers/relay
- [sync-lan-transport](../sync-lan-transport/notes.md) ·
  [sync-protocol-frames](../sync-protocol-frames/notes.md) ·
  [sync-crypto-envelope](../sync-crypto-envelope/notes.md).
