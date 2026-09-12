//! Capability tokens (plan M6, design §6.2): `TokenCreate`/`TokenList`/`TokenRevoke`. Owned
//! end-to-end by the token-system task; the RPCs delegate here from `server.rs` untouched.
//!
//! Scope grammar (design §6.2): five write scopes, `read`, `raw`, and three restrictor prefixes
//! each needing a non-empty suffix (`project:+work`, not bare `project:`). Anything else is
//! rejected at create time — a scope the daemon doesn't recognize must never silently succeed.
//! `expires` is RFC 3339 text on the wire (empty = never expires); parsed with `humantime`.
//! https://docs.rs/humantime
//!
//! The bearer secret is 32 bytes of OS entropy, returned in plaintext exactly once — this RPC's
//! response — and never again; only its hash lands in `txtodo-store` (`create_token`).
//! Request-time enforcement (refusing a revoked/expired bearer on every call) is the larger plan
//! M6 MCP-auth-server milestone: `Store::verify_token` is the primitive it will call, exercised
//! by `tokens_tests.rs` but not reachable over gRPC yet.

use crate::server::TxtodoService;
use std::sync::PoisonError;
use std::time::{Duration, UNIX_EPOCH};
use tonic::{Request, Response, Status};
use txtodo_model::{TokenId, Ulid};
use txtodo_proto::v1 as pb;
use txtodo_store::{NewToken, TokenRecord};

/// The design §6.2 exact-match scopes; restrictor prefixes are checked separately.
const EXACT_SCOPES: [&str; 6] = [
    "read",
    "write:add",
    "write:complete",
    "write:edit",
    "write:delete",
    "raw",
];
/// Restrictor caveat prefixes; each needs a non-empty suffix.
const RESTRICTOR_PREFIXES: [&str; 3] = ["project:", "context:", "file:"];

/// True for a scope string in the design §6.2 closed union.
fn is_valid_scope(scope: &str) -> bool {
    EXACT_SCOPES.contains(&scope)
        || RESTRICTOR_PREFIXES
            .iter()
            .any(|p| scope.strip_prefix(p).is_some_and(|rest| !rest.is_empty()))
}

/// The first scope `req.scopes` carries that is not in the closed union, if any.
fn first_invalid_scope(scopes: &[String]) -> Option<&str> {
    scopes
        .iter()
        .map(String::as_str)
        .find(|s| !is_valid_scope(s))
}

/// 32 bytes of OS entropy, hex-encoded: the bearer value returned exactly once, at creation.
/// https://docs.rs/getrandom — the same CSPRNG `clock.rs` uses for ULID entropy.
fn generate_secret() -> Result<String, Status> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|_| Status::internal("entropy source failed"))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

/// An RFC 3339 `expires` string into Unix ms; empty means "never expires".
fn parse_expires(s: &str) -> Result<Option<u64>, Status> {
    if s.is_empty() {
        return Ok(None);
    }
    let when = humantime::parse_rfc3339(s)
        .map_err(|e| Status::invalid_argument(format!("expires {s:?}: {e}")))?;
    let ms = when
        .duration_since(UNIX_EPOCH)
        .map_err(|_| Status::invalid_argument("expires predates the Unix epoch"))?
        .as_millis();
    u64::try_from(ms)
        .map(Some)
        .map_err(|_| Status::invalid_argument("expires is out of range"))
}

/// Unix ms back to RFC 3339 for the wire; `None` is the empty string (never expires).
fn format_expires(ms: Option<u64>) -> String {
    let Some(ms) = ms else {
        return String::new();
    };
    humantime::format_rfc3339_millis(UNIX_EPOCH + Duration::from_millis(ms)).to_string()
}

/// A wire token id (ULID text) into the typed id.
fn parse_token_id(s: &str) -> Result<TokenId, Status> {
    Ulid::parse(s)
        .map(TokenId::new)
        .ok_or_else(|| Status::invalid_argument(format!("{s:?} is not a token id")))
}

/// A stored record onto the wire; `secret` is empty outside `TokenCreate`'s own response.
fn to_pb(t: TokenRecord) -> pb::Token {
    pb::Token {
        id: t.id.ulid().to_string(),
        name: t.name,
        scopes: t.scopes,
        expires: format_expires(t.expires_at_ms),
        created_at_ms: t.created_at_ms,
        secret: String::new(),
    }
}

impl TxtodoService {
    /// Mints a new capability token from the design §6.2 scope/caveat grammar.
    pub(crate) async fn token_create_impl(
        &self,
        r: Request<pb::TokenCreateRequest>,
    ) -> Result<Response<pb::Token>, Status> {
        let req = r.into_inner();
        if req.name.trim().is_empty() {
            return Err(Status::invalid_argument("token name must not be empty"));
        }
        if req.scopes.is_empty() {
            return Err(Status::invalid_argument("a token needs at least one scope"));
        }
        if let Some(bad) = first_invalid_scope(&req.scopes) {
            return Err(Status::invalid_argument(format!(
                "{bad:?} is not a recognized scope (design §6.2)"
            )));
        }
        let expires_at_ms = parse_expires(&req.expires)?;
        let secret = generate_secret()?;
        let ws = self.workspace();
        let clock = ws.clock();
        let new = NewToken {
            id: TokenId::new(clock.new_ulid()),
            name: req.name,
            scopes: req.scopes,
            secret: secret.clone(),
            created_at_ms: clock.now_ms(),
            expires_at_ms,
        };
        ws.store()
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .create_token(&new)
            .map_err(|e| Status::internal(e.to_string()))?;
        Ok(Response::new(pb::Token {
            id: new.id.ulid().to_string(),
            name: new.name,
            scopes: new.scopes,
            expires: format_expires(new.expires_at_ms),
            created_at_ms: new.created_at_ms,
            secret,
        }))
    }

    /// Lists tokens for this workspace, scopes included, secrets never returned. Revoked tokens
    /// disappear from the list: the wire `Token` message carries no revoked marker to show them
    /// with, so a revoked row would be indistinguishable from a live one if it stayed.
    pub(crate) async fn token_list_impl(
        &self,
        _r: Request<pb::TokenListRequest>,
    ) -> Result<Response<pb::TokenListResponse>, Status> {
        let records = {
            let ws = self.workspace();
            let store = ws.store().lock().unwrap_or_else(PoisonError::into_inner);
            store
                .list_tokens()
                .map_err(|e| Status::internal(e.to_string()))?
        };
        let tokens = records
            .into_iter()
            .filter(|t| t.revoked_at_ms.is_none())
            .map(to_pb)
            .collect();
        Ok(Response::new(pb::TokenListResponse { tokens }))
    }

    /// Revokes a token; the daemon refuses it on its next use (enforced by a future request-time
    /// auth path — `Store::verify_token` is where that refusal will come from).
    pub(crate) async fn token_revoke_impl(
        &self,
        r: Request<pb::TokenRevokeRequest>,
    ) -> Result<Response<pb::TokenRevokeResponse>, Status> {
        let id = parse_token_id(&r.get_ref().id)?;
        let ws = self.workspace();
        let now_ms = ws.clock().now_ms();
        let revoked = ws
            .store()
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .revoke_token(id, now_ms)
            .map_err(|e| Status::internal(e.to_string()))?;
        Ok(Response::new(pb::TokenRevokeResponse { revoked }))
    }
}
