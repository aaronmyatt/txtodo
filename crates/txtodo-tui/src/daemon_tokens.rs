//! Agent tokens (task `tui-revamp/tui-foundation`), for Settings › Tokens. Split out of
//! `daemon.rs` by area. The secret comes back once, from `token_create`, and is never logged.

use txtodo_proto::v1 as pb;

use crate::daemon::{Daemon, DaemonError};

impl Daemon {
    /// Mints a token: `scopes` from the closed union (`read`, `write:*`, `raw`, restrictors),
    /// `expires` RFC 3339 or empty for none.
    pub async fn token_create(
        &mut self,
        name: &str,
        scopes: Vec<String>,
        expires: &str,
    ) -> Result<pb::Token, DaemonError> {
        let req = pb::TokenCreateRequest {
            name: name.to_owned(),
            scopes,
            expires: expires.to_owned(),
            workspace: self.selector.clone(),
        };
        Ok(self.inner.token_create(req).await?.into_inner())
    }

    /// Every live token, secrets never included.
    pub async fn token_list(&mut self) -> Result<pb::TokenListResponse, DaemonError> {
        let req = pb::TokenListRequest {
            workspace: self.selector.clone(),
        };
        Ok(self.inner.token_list(req).await?.into_inner())
    }

    /// Revokes one token.
    pub async fn token_revoke(&mut self, id: &str) -> Result<pb::TokenRevokeResponse, DaemonError> {
        let req = pb::TokenRevokeRequest {
            id: id.to_owned(),
            workspace: self.selector.clone(),
        };
        Ok(self.inner.token_revoke(req).await?.into_inner())
    }
}
