# Macaroon-style agent tokens: mint, attenuate, verify, revoke (plan M6, plan §6)

## Goal

Signed root tokens with ordered attenuating caveats (design §6.2). A token holder can mint a
*narrower* token but never a broader one. The CLI surface is `txtodo token
create|list|revoke|attenuate`. Scopes from design §6.2: `read`, `write:add`, `write:complete`,
`write:edit`, `write:delete`, `raw`, plus the restricting caveats `project:+x`, `context:@y`,
`file:work.txt`. Plan M6 names the six caveat kinds: `scope=…`, `project=…`, `context=…`,
`file=…`, `expires=…`, `quarantine=@ctx`.

## Design

### The token model — pure, in `txtodo-mcp::token`

`txtodo-mcp` may only depend on `txtodo-proto` + `txtodo-query` (`budgets.json.allowedDeps`), so
the crypto is pure: no store, no keystore, no clock. The daemon (which *may* import `txtodo-mcp`)
owns the root key and the revocation set and passes them into the pure functions — the same shape
`txtodo-mcp` already uses for the `McpBackend` trait in
[mcp-server-tools](../mcp-server-tools/notes.md).

```rust
// crates/txtodo-mcp/src/token.rs — no deps beyond std/hmac; the daemon supplies the secrets.
pub struct Token {
    pub id: TokenId,           // ULID (model ids.rs) — sorts by creation in `token list`
    pub root_key_id: RootKeyId,
    pub caveats: Vec<Caveat>,  // ordered; the HMAC chain depends on this order
    pub sig: [u8; 32],         // last link of the chained HMAC
}

pub enum Caveat {
    Scope(ScopeSet),        // scope=read,write:add
    Project(String),        // project=+work
    Context(String),        // context=@laptop
    File(FilePath),         // file=work.txt (model FilePath, validated at parse)
    Expires(u64),           // expires=unix_seconds (daemon clock injected, not read here)
    Quarantine(String),     // quarantine=@inbox — consumed by mcp-agent-principal, add-only
}

pub fn mint(root: &RootKey, id: TokenId, caveats: Vec<Caveat>) -> Token;
pub fn attenuate(t: &Token, c: Caveat) -> Token;   // append one caveat + re-chain the HMAC
pub fn verify(t: &Token, root: &RootKey, revoked: &RevocationSet, now: u64)
    -> Result<Auth, VerifyError>;
```

- `RootKey` is 32 bytes. `RevocationSet` is `&HashSet<TokenId>` (or a `Fn(TokenId) -> bool`); the
  daemon reads both, `txtodo-mcp` only consumes.
- `verify` walks the caveats in order: every `scope=` is intersected (conjunction — narrower only),
  `expires=` is compared against the injected `now`, and `project/context/file` fold into a
  `ScopeFilter` predicate the daemon applies per task. The signature is recomputed from the root
  key, so a holder who edits a caveat produces an invalid chain.
- Attenuation keeps the same `id` (the root id), so `revoke <id>` kills every derivative — the
  intended blast radius. Attenuation only appends: the HMAC chain makes removing a caveat
  impossible without the root key.
- `ScopeSet` is the six capability scopes as an enum set, matching design §6.2 exactly:

```rust
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Scope { Read, WriteAdd, WriteComplete, WriteEdit, WriteDelete, Raw }
```

### Implementation choice (plan M6 leaves it open)

Either the `macaroon` crate (<https://docs.rs/macaroon>) or a minimal HMAC-chained impl on
`hmac` + `sha2` (<https://docs.rs/hmac>, <https://docs.rs/sha2>). The minimal impl is ~120 lines
and keeps the `Caveat` enum as the single source of truth for the wire grammar; the crate adds a
dependency plus its own caveat string format. Record the choice in the `token.rs` doc comment —
whichever is chosen, `verify`'s property (attenuation never widens) is the thing the tests pin.
Reference for the construction: Macaroons paper, <https://research.google/pubs/pub41892/>.

## Placement/dependencies

- New `crates/txtodo-mcp/src/token.rs` (plus `token_tests.rs`). Pure; fits the allowedDeps.
- New dep (`macaroon` *or* `hmac` + `sha2`): human sign-off + `cargo deny check` pass
  (`deny.toml` frozen) before any code lands. If the workspace centralises deps in the root
  `Cargo.toml` `[workspace.dependencies]`, adding one touches a frozen path → ask.
- Root secret: the keystore gains a `KeyId::TokenRoot` variant (the `KeyId` enum is
  `{ DeviceSigning, DeviceStatic, Group(epoch) }` — see
  [sync-keystore](../sync-keystore/notes.md)). The daemon `KeyStore::get(KeyId::TokenRoot)` returns
  a `Secret` (zeroized on drop, redacted `Debug`); its bytes are passed to `mint`/`verify` and
  never logged.
- Revocation set and token metadata live in the store `meta` table (key/value BLOB,
  `crates/txtodo-store/migrations/0001.sql`): a `token_revocations` key holding the postcard-encoded
  `TokenId` set, and `token:<id>` entries holding `{ name, caveats, created_at }` for `list`. If
  the metadata set grows past a handful of entries, promote to a `tokens` table (a store migration,
  committed alone) — explicit max either way (constitution §1).
- Wire: `crates/txtodo-proto/proto/txtodo/v1/txtodo.proto` gains `TokenCreate`, `TokenList`,
  `TokenRevoke`, `TokenAttenuate` RPCs on the `Txtodo` service plus `Token*` messages. Generated
  output is a `generatedPaths` artifact (`crates/*/src/generated/**`), diff-budget exempt, committed
  alone. `crates/txtodo-cli` adds the `txtodo token …` subcommand over gRPC.

## Edge cases & invariants

- **Never widen (asserted negative).** The property test mints `{read, write:add}`, attenuates with
  `scope=read`, and asserts the derived token's effective scope is `{read}` — and that attenuating
  with a *broader* scope is impossible (the enum set is closed; there is no "wider" value to add).
- Caveat order is canonical for the HMAC chain: serialize in enum order, never insertion order, so
  the same caveats always chain to the same signature.
- `expires` is `unix_seconds`; the daemon injects its `clock.rs` clock so tests use fake time.
- `project/context/file` caveat values are parsed at the boundary (`project` must start with `+`,
  `context` with `@`, `file` via `FilePath::new`) — a malformed caveat is a CLI error, never a
  silently-ignored restriction.
- Token text is a secret: the `Token` type's `Debug`/`Display` redacts the signature and caveats
  (mcp-auth logs `token_id` only). Same discipline as `Secret` (CLAUDE.md §3.1).
- `quarantine=@ctx` is validated as a context here but *consumed* by mcp-agent-principal — the token
  layer never appends tags.

## Acceptance

- `mint → attenuate → verify` round-trips to the exact narrowed `ScopeSet` + `ScopeFilter`.
- An attenuated token cannot widen: verify returns the intersection, never a superset (property
  test over random caveat sequences, `proptest`).
- An expired token fails `verify` with `VerifyError::Expired`; a revoked token (and every derivative
  sharing its id) fails with `VerifyError::Revoked`; a tampered caveat fails the HMAC chain.
- `txtodo token create` mints a working token; `list` shows it; `attenuate` yields a narrower
  working token; `revoke` makes all of them fail `verify`.
- `cargo deny check` passes for the new dep.

## Frozen paths touched

- `Cargo.lock`: new crate(s) — lockfile update is a generated artifact, committed alone.
- Root `Cargo.toml`: only if `[workspace.dependencies]` gains the entry — ask, never silent.

## References

- plan M6 (txtodo-implementation-plan.md), design §6.2 (txtodo-design.md)
- <https://docs.rs/macaroon> · <https://docs.rs/hmac> · <https://docs.rs/sha2>
- Macaroons paper: <https://research.google/pubs/pub41892/>
- keystore `KeyId`/`Secret`: [../sync-keystore/notes.md](../sync-keystore/notes.md)
- proto guide: <https://protobuf.dev/programming-guides/proto3/>
