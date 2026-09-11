# Enable iroh relay and hole-punching (plan M8)

Flips the M4 decision. [sync-lan-transport](../sync-lan-transport/notes.md) built the iroh QUIC
endpoint with relay **explicitly disabled**; design §4.5 ("Direct internet" / "Relay" rows) and plan
M8 turn it on. Refs: <https://www.iroh.computer/docs> · <https://docs.rs/iroh>.

## What "enable" means

- Hole-punching is iroh's default for direct connections; the relay is rendezvous + fallback when
  punch fails or a peer is asleep. Both come from the same endpoint builder.
- The relay only sees ciphertext — ops are already encrypted + signed
  ([sync-crypto-envelope](../sync-crypto-envelope/notes.md)). This wires the transport, not crypto.
- Relay stays **optional** (design §4.5). Config is a relay URL from `config.toml` / `--relay`;
  empty ⇒ relay off, M4 behaviour unchanged.

## One constructor, mode explicit; `Link` still holds

The relay set is exactly what config says, in one constructor nothing bypasses — relay-on ⇒
configured set non-empty, relay-off ⇒ empty (the test that catches an iroh upgrade changing
defaults). The session machine never sees a socket ([sync-lan-transport](../sync-lan-transport/notes.md)
`trait Link`); hole-punch/relay is another `Link` impl, iroh in one module, nowhere else.

## Bounds and failure

`MAX_RELAY_PEERS`, asserted; a relay endpoint is a peer table, unbounded unless capped. Named
connection/rendezvous timeouts; bounded backoff with an explicit iteration cap. Network input is
validated, never asserted. `txtodo doctor` gains the configured relay URL and last punch/relay
outcome.

## Tests

- Relay-on endpoint has a non-empty configured relay set; relay-off has an empty one.
- Two in-process endpoints rendezvous through a local iroh relay and exchange a `Hello` within a
  bounded time (callback-driven, timeout fails CI, no sleeps).
- A peer in another group is never connected to.
