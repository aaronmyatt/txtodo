//! Loads-or-mints workspace-lifetime state that lives in this workspace's own `meta` table
//! (identity mode only, since ADR 0021 — device id and sync group moved to a shared
//! `DeviceIdentity`, `device_identity.rs`). Split out of `workspace.rs` to keep that file within
//! its line budget.

use crate::workspace_error::WorkspaceError;
use txtodo_model::{FilePath, IdentityMode};
use txtodo_store::Store;

/// The `meta` key holding this workspace's identity mode (docs/questions.md Q2).
pub(crate) const IDENTITY_MODE_KEY: &str = "identity_mode";

/// The last `/`-separated segment of a workspace-relative path.
pub(crate) fn basename(path: &FilePath) -> &str {
    path.as_str().rsplit('/').next().unwrap_or(path.as_str())
}

/// Loads this workspace's identity mode from `meta`, or mints `default` (`txtodod
/// --identity-mode`, itself `Sidecar`: ADR 0019). Fixed for the workspace's lifetime once minted,
/// like the device/group id. A new workspace used to mint `Tagged` when any of its lists already
/// carried an `id:` tag, and Tagged stamps `id:` into every line: registering an old checkout of
/// a backlog rewrote ~120 lists (task cli-no-implicit-register). The `id:` text a line still
/// carries is kept as it is, never added to.
pub(crate) fn load_or_mint_identity_mode(
    store: &mut Store,
    default: IdentityMode,
) -> Result<IdentityMode, WorkspaceError> {
    if let Some(bytes) = store.meta_get(IDENTITY_MODE_KEY)?
        && let Some(mode) = decode_identity_mode(&bytes)
    {
        return Ok(mode);
    }
    store.meta_set(IDENTITY_MODE_KEY, &encode_identity_mode(default))?;
    Ok(default)
}

pub(crate) fn encode_identity_mode(mode: IdentityMode) -> [u8; 1] {
    match mode {
        IdentityMode::Tagged => [0],
        IdentityMode::Sidecar => [1],
    }
}

fn decode_identity_mode(bytes: &[u8]) -> Option<IdentityMode> {
    match bytes {
        [0] => Some(IdentityMode::Tagged),
        [1] => Some(IdentityMode::Sidecar),
        _ => None,
    }
}
