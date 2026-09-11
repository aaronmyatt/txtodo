# MCP auth: bearer on HTTP, inherited token on stdio (plan M6, plan §6)

## Goal

Every MCP request carries an authenticated principal. On Streamable HTTP the token rides
`Authorization: Bearer <token>` (design §6.1/§6.2); on stdio the subprocess inherits a token from
`--token` or the config's `default_stdio_token` (plan M6). The resolved identity — token id, name,
scope, quarantine context — is attached to the request so every handler and mutation reads it once,
never re-parses it.

## Design

### The verifier seam — keeps `txtodo-mcp` inside its allowedDeps

`txtodo-mcp` owns the transport-level auth types and the pure verify call; the daemon owns the root
key + revocation set and implements the lookup. Same split as the `McpBackend` trait.

```rust
// crates/txtodo-mcp/src/auth.rs
pub struct AuthContext {
    pub token_id: TokenId,        // model ids.rs
    pub name: String,             // for the op-log principal (agent:name@device)
    pub scope: ScopeSet,          // resolved by token::verify (mcp-tokens)
    pub filter: ScopeFilter,      // project/context/file caveats, applied per task
    pub quarantine: Option<String>, // from quarantine=@ctx, consumed by mcp-agent-principal
}

pub enum AuthError {
    Missing,                      // no Authorization header / no stdio token
    Malformed,                    // not "Bearer <token>" / token text fails to parse
    Expired,                      // expires= caveat in the past (daemon clock)
    Revoked,                      // id in the revocation set
    InsufficientScope(&'static str), // present and valid, but lacks the scope a tool needs
}

// The daemon supplies this; txtodo-mcp's HTTP/stdio glue calls it, never the store directly.
pub trait TokenVerifier: Send + Sync {
    fn verify(&self, token: &Token, now: u64) -> Result<AuthContext, AuthError>;
}
```

### HTTP (Streamable HTTP on `127.0.0.1:8636/mcp`, design §6.1)

- A middleware wraps every `rmcp` request: read `Authorization`, require the `Bearer ` prefix
  (RFC 6750 <https://www.rfc-editor.org/rfc/rfc6750>), call `TokenVerifier::verify`, and attach the
  `AuthContext` to the request. Missing/malformed/expired/revoked → HTTP 401 with a
  `WWW-Authenticate: Bearer` challenge. Insufficient scope → 403.
- Verification happens once per request, before the `mcp-server-tools` dispatch, so the
  scope check in that task reads `AuthContext.scope` rather than re-deriving it.

```rust
// crates/txtodo-mcp/src/auth.rs — HTTP middleware
pub fn authenticate(header: Option<&str>, verifier: &dyn TokenVerifier, now: u64)
    -> Result<AuthContext, AuthError>;
```

### stdio (`txtodo mcp --stdio`)

- The token is ambient: resolved once at server start from `--token`, else the config's
  `default_stdio_token`, else the server refuses to start with a typed error (an MCP agent always
  acts as *some* token — design §6.2 records the token's principal on every mutation).
- Precedence: explicit `--token` wins over `default_stdio_token`; both absent is an error, not a
  silent "serve as the user".

```rust
// crates/txtodo-cli/src/config.rs — additive field, no other precedence changes
#[serde(deny_unknown_fields)]
pub struct Config {
    // …existing fields…
    /// Token used by `txtodo mcp --stdio` when `--token` is not passed.
    pub default_stdio_token: Option<String>,
}
```

- `Config` already treats absent as default (design §2.2 rule 4); the field is `Option`, so a config
  without it still parses. `deny_unknown_fields` means the key must be added to the schema before a
  user can set it — additive, not breaking.

## Placement/dependencies

- `crates/txtodo-mcp/src/auth.rs` (`AuthContext`, `AuthError`, `TokenVerifier`, the HTTP
  middleware) and the stdio wiring in the server entry (the `txtodo mcp` subcommand). No new
  crates; `rmcp` is already a dep from mcp-server-tools.
- `crates/txtodo-daemon` implements `TokenVerifier` against its keystore root key + store revocation
  set (both owned there per mcp-tokens) and its `clock.rs` clock.
- `crates/txtodo-cli/src/config.rs` gains one field; `crates/txtodo-cli` parses `--token`.

## Edge cases & invariants

- **Never log the token.** Every log line carries `token_id` only; a test captures `tracing` output
  across auth and asserts no token text, root-key bytes, or caveat values appear (shared with
  security-m6-review).
- Malformed header (missing `Bearer ` prefix, non-token garbage) is `Malformed → 401`, distinct
  from `Expired`/`Revoked` so a human can tell "typo" from "dead token".
- The `now` used for `expires` is the daemon's injected clock — tests drive fake time, no sleeps.
- stdio has no per-request header; its single `AuthContext` is fixed at start. A revoked token on an
  already-started stdio server is re-checked on the next verification (verify runs per request even
  on stdio, so a mid-session revoke takes effect).
- Insufficient scope is 403 (authenticated but not allowed), never 401 — a client that retries with
  a better token can tell the difference.

## Acceptance

- A valid Bearer token is accepted and yields the expected `AuthContext` (scope + filter +
  quarantine).
- Garbage, revoked, and expired tokens return 401; a valid token lacking a required scope returns
  403.
- stdio without `--token` uses `default_stdio_token`; explicit `--token` wins; neither → the server
  refuses to start.
- Logs across token create + auth + a tool call contain `token_id` but never token text or caveats.

## References

- plan M6 (txtodo-implementation-plan.md), design §6.1/§6.2 (txtodo-design.md)
- MCP transports + auth: <https://modelcontextprotocol.io/specification/2025-06-18/basic/transports>
- Bearer: RFC 6750 <https://www.rfc-editor.org/rfc/rfc6750>
- tokens: [../mcp-tokens/notes.md](../mcp-tokens/notes.md) · scope check: [../mcp-server-tools/notes.md](../mcp-server-tools/notes.md)
