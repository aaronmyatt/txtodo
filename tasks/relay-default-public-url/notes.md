# Default relay URL: dogfood on n0's public relay, no self-hosting

## Goal

Cross-network sync (devices that never share a LAN) needs a relay reachable at an explicit URL —
`crates/txtodo-sync/src/relay.rs:67` builds `RelayMode::Custom` from exactly one `cfg.url`, with no
built-in fallback to any public relay map. Today that means `--relay <url>` (or the env/config
equivalent) is mandatory before two devices on different networks can sync at all.

This ticket removes that requirement for the common case: default to a real, already-proven public
relay so sync works out of the box, with self-hosting kept as a drop-in override for later (e.g.
if privacy, reliability at scale, or access control ever becomes a real requirement — see
`relay-kamal-deploy`/`relay-accounts-service`, both deliberately out of scope here).

## Why `https://use1-1.relay.n0.iroh.link` specifically

Not a guess — it's the exact URL this repo's own integration tests already dial successfully and
repeatedly: `crates/txtodo-daemon/tests/relay_converge.rs:62` and
`crates/txtodo-daemon/tests/pairing_relay.rs:42` both hard-code it as `RELAY_URL`, documented as
"one of iroh's own documented public relay servers" (named in `docs.rs/iroh`'s own
`Endpoint::builder` doctest). It's third-party-operated (n0, iroh's maintainer), not anything this
project runs.

## Why this is safe to dogfood on

Every `Op` is AEAD-sealed (XChaCha20-Poly1305) and Ed25519-signed before it ever reaches any relay
transport (design §4.6, `sync-crypto-envelope`) — a relay only ever routes ciphertext, never
plaintext task content. This is the identical threat model ADR 0018/0027 already accepted for a
*self-hosted* relay; using n0's public one instead changes who operates the box, not what it can
see. The one known, narrower gap (relay op-type frame-length side channel, `id:06G9ZV2JPBJ8P829RA634BE8KG`,
already tracked, deferred) applies equally either way.

## Scope

- A `DEFAULT_RELAY_URL` constant, used only when no explicit `--relay`/`$TXTODO_RELAY_URL`/
  `config.toml relay_url` is set — the existing precedence chain doesn't change shape, it just
  gains a fourth, final fallback instead of erroring or leaving relay disabled.
- `txtodo doctor` says which URL is in effect and whether it's the default or an override, so a
  self-hoster later isn't left guessing whether their override actually took.
- Docs updated to stop implying self-hosting is required for cross-network sync.

## Explicitly not this ticket

- No access control, no allowlist, no accounts service — n0's public relay has its own operational
  policies, not this project's. If those ever prove insufficient, `relay-kamal-deploy` and
  `relay-accounts-service` (both parked, not touched by this pass) are the escalation path.
- No change to LAN behavior (`RelayMode::Disabled` for the LAN endpoint) or to the relay-dial bugs
  tracked separately as `pairing-workspace-identity` and `sync-pairing-relay-ongoing-dial` — this
  ticket only removes the need to configure a URL; it does not fix convergence itself.
