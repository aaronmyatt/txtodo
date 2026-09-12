-- Capability tokens (plan M6, design §6.2): macaroon-style scoped bearer tokens for MCP agents
-- and the desktop devices screen. Only the blake3 hash of the bearer secret is stored, never the
-- secret itself, so a leaked database cannot mint a valid bearer value; the plaintext is handed
-- back to the caller exactly once, at creation. `scopes` is postcard of a `Vec<String>` (the
-- crate's one payload codec, same idiom as `ops.payload`). Revocation follows the crate's upsert
-- idiom (`tokens.rs`'s `revoke_token`, mirroring `clear_flag`): `revoked_at` NULL means live, so
-- there is still no bare `UPDATE` statement of this crate's own.
-- https://www.sqlite.org/lang_createtable.html · https://www.sqlite.org/pragma.html#pragma_user_version
CREATE TABLE tokens (
    id          BLOB PRIMARY KEY,
    name        TEXT NOT NULL,
    scopes      BLOB NOT NULL,
    secret_hash BLOB NOT NULL,
    created_at  INTEGER NOT NULL,
    expires_at  INTEGER,
    revoked_at  INTEGER
);
CREATE UNIQUE INDEX tokens_secret_hash ON tokens(secret_hash);
PRAGMA user_version = 4;
