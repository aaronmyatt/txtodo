# Macaroon-style agent tokens (plan M6, plan §6)

Goal. Signed root tokens with attenuating caveats; a holder mints narrower tokens, never
broader. `txtodo token create|list|revoke|attenuate` (design §6.2).

## Model (txtodo-mcp::token)
- Macaroon: root id + location + ordered caveats + a chained HMAC signature.
- Caveats: `scope=…`, `project=…`, `context=…`, `file=…`, `expires=…`, `quarantine=@ctx`.
- `mint(root_key, caveats) -> Token`, `attenuate(&token, caveat) -> Token`,
  `verify(token, root_key, revocation) -> Scope`.
- Verify evaluates every caveat; a token cannot widen scope, and attenuation only adds
  restrictions — this is the property the tests pin.

## Implementation choice (plan M6)
- `macaroon` crate or a minimal HMAC-chained impl in `txtodo-mcp::token` — the pure
  crypto has no store/keystore dependency, so it fits the crate's boundary
  (txtodo-proto, txtodo-query only).
- https://docs.rs/macaroon, https://docs.rs/hmac

## Where the secrets live
- Root secret in the keystore (sync-keystore, M4); revocation list in the store
  (`meta` table). The daemon owns both and passes them into `token`'s pure functions —
  `txtodo-mcp` never opens the keystore or store itself.

## CLI + wire
- `txtodo token create|list|revoke|attenuate` (crates/txtodo-cli) talks gRPC; new proto
  messages/RPCs in txtodo.proto (regenerated artifact, committed alone).

## Acceptance
- mint→attenuate→verify round-trip; attenuated token cannot widen; expired token fails;
  revoked token fails (design §6.2).
