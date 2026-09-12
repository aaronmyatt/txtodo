//! Capability token DTO (plan M6, design §6.2): mirrors `pb::Token`. `secret` is the bearer value,
//! present only in `TokenCreate`'s own response — `TokenList` always sends it empty, so the
//! frontend never has to remember to blank it out itself. Split out of `dto.rs`; see that file's
//! module doc.

use serde::Serialize;
use txtodo_proto::v1 as pb;

/// One capability token (`TokenCreate`/`TokenList`).
#[derive(Debug, Clone, Serialize)]
pub struct TokenDto {
    /// ULID text.
    pub id: String,
    /// Human-chosen label.
    pub name: String,
    /// Closed union (design §6.2): `read`, `write:*`, `raw`, `project:`/`context:`/`file:` + suffix.
    pub scopes: Vec<String>,
    /// RFC 3339; empty = no expiry.
    pub expires: String,
    /// Unix ms.
    pub created_at_ms: u64,
    /// Bearer secret in plaintext; non-empty only in `TokenCreate`'s response.
    pub secret: String,
}

impl From<pb::Token> for TokenDto {
    fn from(t: pb::Token) -> TokenDto {
        TokenDto {
            id: t.id,
            name: t.name,
            scopes: t.scopes,
            expires: t.expires,
            created_at_ms: t.created_at_ms,
            secret: t.secret,
        }
    }
}
