//! Capability token commands (plan M6, design §6.2): `TokenCreate`/`TokenList`/`TokenRevoke`.
//! Split out of `commands.rs` the way `crates/txtodo-daemon/src/server.rs` delegates to
//! `tokens.rs`; same `ensure_connected` + lock + map-err-to-String pattern as every command there.

use crate::commands::ensure_connected;
use crate::dto::TokenDto;
use crate::state::AppState;
use tauri::{AppHandle, State};
use txtodo_proto::v1 as pb;

/// Mints a new capability token from the design §6.2 scope/caveat grammar; the daemon rejects an
/// unrecognized scope at create time. `expires` is RFC 3339 text; empty means no expiry.
#[tracing::instrument(name = "ipc.token_create", skip_all)]
#[tauri::command]
pub async fn token_create(
    app: AppHandle,
    state: State<'_, AppState>,
    name: String,
    scopes: Vec<String>,
    expires: String,
) -> Result<TokenDto, String> {
    token_create_inner(app, state, name, scopes, expires).await
}

async fn token_create_inner(
    app: AppHandle,
    state: State<'_, AppState>,
    name: String,
    scopes: Vec<String>,
    expires: String,
) -> Result<TokenDto, String> {
    ensure_connected(&app, &state).await?;
    let mut client = state.client_snapshot().await?;
    let req = pb::TokenCreateRequest {
        name,
        scopes,
        expires,
        workspace: None,
    };
    let resp = client.token_create(req).await.map_err(|e| e.to_string())?;
    Ok(TokenDto::from(resp))
}

/// Tokens for this workspace, scopes included; a revoked token has already dropped out of this
/// list (the wire message carries no revoked marker to show it with).
#[tracing::instrument(name = "ipc.token_list", skip_all)]
#[tauri::command]
pub async fn token_list(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Vec<TokenDto>, String> {
    token_list_inner(app, state).await
}

async fn token_list_inner(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Vec<TokenDto>, String> {
    ensure_connected(&app, &state).await?;
    let mut client = state.client_snapshot().await?;
    let resp = client.token_list().await.map_err(|e| e.to_string())?;
    Ok(resp.tokens.into_iter().map(TokenDto::from).collect())
}

/// Revokes a token by id; the daemon refuses it on its next use.
#[tracing::instrument(name = "ipc.token_revoke", skip_all)]
#[tauri::command]
pub async fn token_revoke(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> Result<bool, String> {
    token_revoke_inner(app, state, id).await
}

async fn token_revoke_inner(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> Result<bool, String> {
    ensure_connected(&app, &state).await?;
    let mut client = state.client_snapshot().await?;
    let resp = client.token_revoke(id).await.map_err(|e| e.to_string())?;
    Ok(resp.revoked)
}
