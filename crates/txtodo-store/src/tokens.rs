//! Capability tokens (plan M6, design §6.2): macaroon-style scoped bearer tokens for MCP agents
//! and the desktop devices screen. `scopes` is stored verbatim — the caller (the daemon's gRPC
//! boundary) validates against the design's closed union before it ever reaches here. Only
//! `blake3(secret)` is stored, never the secret; `verify_token` is the enforcement primitive a
//! future request-time agent-auth path calls, nothing on the wire calls it yet (that path is
//! plan M6's larger MCP server, out of scope for this crate). Revocation follows the crate's
//! upsert idiom (`clear_flag`'s pattern): `revoked_at` NULL means live, so there is still no bare
//! `UPDATE` statement here. No clock in this crate (constitution §3): every timestamp is the
//! caller's `_ms` argument, mirroring `Hlc::tick(now_ms)` in txtodo-model.
//! Ref: https://docs.rs/blake3 · https://www.sqlite.org/lang_upsert.html

use std::fmt;

use rusqlite::{OptionalExtension, params};
use txtodo_model::{TokenId, Ulid};

use crate::ops::wall_i64;
use crate::{Store, StoreError};

/// Most tokens one `list_tokens` read returns; a human workspace never approaches this.
pub const MAX_TOKENS_PER_READ: usize = 1_024;

/// Everything `create_token` needs, bundled so the call stays under the arg-count budget.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NewToken {
    /// The token's identity (minted by the caller; a ULID so it sorts by creation time).
    pub id: TokenId,
    /// Human label ("claude-code").
    pub name: String,
    /// Closed-union scope strings (design §6.2); the caller has already validated these.
    pub scopes: Vec<String>,
    /// The bearer secret in plaintext; hashed here and never stored as given.
    pub secret: String,
    /// Unix milliseconds when the token was minted.
    pub created_at_ms: u64,
    /// Unix milliseconds after which the token is refused; `None` never expires.
    pub expires_at_ms: Option<u64>,
}

/// One capability token as stored, minus the secret (never round-tripped once hashed).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TokenRecord {
    /// The token's identity.
    pub id: TokenId,
    /// Human label.
    pub name: String,
    /// Closed-union scope strings, as stored.
    pub scopes: Vec<String>,
    /// Unix milliseconds when the token was minted.
    pub created_at_ms: u64,
    /// Unix milliseconds after which the token is refused; `None` never expires.
    pub expires_at_ms: Option<u64>,
    /// Unix milliseconds the token was revoked at, if it was.
    pub revoked_at_ms: Option<u64>,
}

/// Why `verify_token` refused a presented secret; not a storage failure.
#[derive(Debug)]
pub enum TokenError {
    /// A storage-layer failure (I/O, decode).
    Store(StoreError),
    /// No token hashes to the presented secret.
    NotFound,
    /// The token exists but was revoked.
    Revoked,
    /// The token exists but its expiry has passed.
    Expired,
}

impl fmt::Display for TokenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TokenError::Store(e) => write!(f, "{e}"),
            TokenError::NotFound => write!(f, "no token matches the presented secret"),
            TokenError::Revoked => write!(f, "token is revoked"),
            TokenError::Expired => write!(f, "token has expired"),
        }
    }
}

impl std::error::Error for TokenError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            TokenError::Store(e) => Some(e),
            TokenError::NotFound | TokenError::Revoked | TokenError::Expired => None,
        }
    }
}

impl From<StoreError> for TokenError {
    fn from(e: StoreError) -> TokenError {
        TokenError::Store(e)
    }
}

fn id_blob(id: TokenId) -> Vec<u8> {
    id.ulid().to_u128().to_be_bytes().to_vec()
}

fn id_of(blob: &[u8]) -> Option<TokenId> {
    let bytes: [u8; 16] = blob.try_into().ok()?;
    Some(TokenId::new(Ulid::from_u128(u128::from_be_bytes(bytes))))
}

/// blake3 of the bearer secret; the only form ever stored or compared.
fn hash_secret(secret: &str) -> Vec<u8> {
    blake3::hash(secret.as_bytes()).as_bytes().to_vec()
}

fn encode_scopes(scopes: &[String]) -> Result<Vec<u8>, StoreError> {
    postcard::to_allocvec(scopes).map_err(StoreError::BadScopes)
}

fn decode_scopes(bytes: &[u8]) -> Result<Vec<String>, StoreError> {
    postcard::from_bytes(bytes).map_err(StoreError::BadScopes)
}

const INSERT_TOKEN: &str = "INSERT INTO tokens (id, name, scopes, secret_hash, created_at, expires_at, revoked_at) \
     VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL)";
const SELECT_ALL: &str = "SELECT id, name, scopes, created_at, expires_at, revoked_at FROM tokens \
     ORDER BY created_at, id LIMIT ?1";
const SELECT_FOR_REVOKE: &str =
    "SELECT name, scopes, secret_hash, created_at, expires_at FROM tokens WHERE id = ?1";
const UPSERT_REVOKE: &str = "INSERT INTO tokens (id, name, scopes, secret_hash, created_at, expires_at, revoked_at) \
     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7) \
     ON CONFLICT(id) DO UPDATE SET revoked_at = excluded.revoked_at";
const SELECT_BY_HASH: &str = "SELECT id, expires_at, revoked_at FROM tokens WHERE secret_hash = ?1";

impl Store {
    /// Mints a token row. `new.scopes` is stored verbatim — the daemon's gRPC boundary is where
    /// the design §6.2 closed union is enforced, once, at parse time.
    pub fn create_token(&mut self, new: &NewToken) -> Result<(), StoreError> {
        let scopes = encode_scopes(&new.scopes)?;
        self.conn
            .execute(
                INSERT_TOKEN,
                params![
                    id_blob(new.id),
                    new.name,
                    scopes,
                    hash_secret(&new.secret),
                    wall_i64(new.created_at_ms),
                    new.expires_at_ms.map(wall_i64),
                ],
            )
            .map_err(StoreError::query("insert token"))?;
        Ok(())
    }

    /// Every token, oldest first, at most `MAX_TOKENS_PER_READ`. Includes revoked and expired
    /// rows — the wire boundary decides what a client is shown.
    pub fn list_tokens(&self) -> Result<Vec<TokenRecord>, StoreError> {
        let mut stmt = self
            .conn
            .prepare_cached(SELECT_ALL)
            .map_err(StoreError::query("prepare list tokens"))?;
        let rows = stmt
            .query_map(params![MAX_TOKENS_PER_READ as i64], |r| {
                Ok((
                    r.get::<_, Vec<u8>>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, Vec<u8>>(2)?,
                    r.get::<_, i64>(3)?,
                    r.get::<_, Option<i64>>(4)?,
                    r.get::<_, Option<i64>>(5)?,
                ))
            })
            .map_err(StoreError::query("query list tokens"))?;
        let mut out = Vec::new();
        for row in rows {
            let (id_bytes, name, scopes_bytes, created_at, expires_at, revoked_at) =
                row.map_err(StoreError::query("read token"))?;
            out.push(TokenRecord {
                id: id_of(&id_bytes).ok_or(StoreError::BadTokenId(id_bytes.len()))?,
                name,
                scopes: decode_scopes(&scopes_bytes)?,
                created_at_ms: u64::try_from(created_at).unwrap_or(0),
                expires_at_ms: expires_at.map(|v| u64::try_from(v).unwrap_or(0)),
                revoked_at_ms: revoked_at.map(|v| u64::try_from(v).unwrap_or(0)),
            });
        }
        debug_assert!(out.len() <= MAX_TOKENS_PER_READ);
        Ok(out)
    }

    /// Marks `id` revoked at `at_ms`. Idempotent: revoking twice just replaces `revoked_at`.
    /// Returns `false` when no such token exists; `true` otherwise.
    pub fn revoke_token(&mut self, id: TokenId, at_ms: u64) -> Result<bool, StoreError> {
        let tx = self
            .conn
            .transaction()
            .map_err(StoreError::query("begin revoke token"))?;
        let existing = tx
            .query_row(SELECT_FOR_REVOKE, params![id_blob(id)], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, Vec<u8>>(1)?,
                    r.get::<_, Vec<u8>>(2)?,
                    r.get::<_, i64>(3)?,
                    r.get::<_, Option<i64>>(4)?,
                ))
            })
            .optional()
            .map_err(StoreError::query("select token for revoke"))?;
        let Some((name, scopes, secret_hash, created_at, expires_at)) = existing else {
            return Ok(false);
        };
        tx.execute(
            UPSERT_REVOKE,
            params![
                id_blob(id),
                name,
                scopes,
                secret_hash,
                created_at,
                expires_at,
                wall_i64(at_ms)
            ],
        )
        .map_err(StoreError::query("revoke token"))?;
        tx.commit().map_err(StoreError::query("commit revoke"))?;
        Ok(true)
    }

    /// Hashes `secret` and resolves the token it belongs to; refuses one that is revoked or
    /// whose `expires_at_ms` is at or before `now_ms`. The enforcement primitive a future
    /// request-time agent-auth path calls — nothing on the wire calls it yet.
    pub fn verify_token(&self, secret: &str, now_ms: u64) -> Result<TokenId, TokenError> {
        let row = self
            .conn
            .query_row(SELECT_BY_HASH, params![hash_secret(secret)], |r| {
                Ok((
                    r.get::<_, Vec<u8>>(0)?,
                    r.get::<_, Option<i64>>(1)?,
                    r.get::<_, Option<i64>>(2)?,
                ))
            })
            .optional()
            .map_err(StoreError::query("select token by hash"))?;
        let Some((id_bytes, expires_at, revoked_at)) = row else {
            return Err(TokenError::NotFound);
        };
        if revoked_at.is_some() {
            return Err(TokenError::Revoked);
        }
        if expires_at.is_some_and(|exp| wall_i64(now_ms) >= exp) {
            return Err(TokenError::Expired);
        }
        id_of(&id_bytes).ok_or_else(|| StoreError::BadTokenId(id_bytes.len()).into())
    }
}
