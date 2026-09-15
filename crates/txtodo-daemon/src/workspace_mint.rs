//! Loads-or-mints workspace-lifetime state that lives in this workspace's own `meta` table
//! (identity mode only, since ADR 0021 — device id and sync group moved to a shared
//! `DeviceIdentity`, `device_identity.rs`). Split out of `workspace.rs` to keep that file within
//! its line budget.

use crate::walker;
use crate::workspace_error::WorkspaceError;
use std::path::Path;
use txtodo_model::{FilePath, IdentityMode};
use txtodo_store::Store;

/// The `meta` key holding this workspace's identity mode (docs/questions.md Q2).
pub(crate) const IDENTITY_MODE_KEY: &str = "identity_mode";

/// The last `/`-separated segment of a workspace-relative path.
pub(crate) fn basename(path: &FilePath) -> &str {
    path.as_str().rsplit('/').next().unwrap_or(path.as_str())
}

/// Loads this workspace's identity mode from `meta`, or mints one: `Tagged` if any discovered
/// document already carries an `id:` tag (no silent mode flip on upgrade, plan decision 3), else
/// `default` (`txtodod --identity-mode`, itself `Sidecar` by default — docs/questions.md Q2, a
/// plain todo.txt needs no `id:` tags written into it). Fixed for the workspace's lifetime once
/// minted, like the device/group id.
pub(crate) fn load_or_mint_identity_mode(
    store: &mut Store,
    root: &Path,
    default: IdentityMode,
) -> Result<IdentityMode, WorkspaceError> {
    if let Some(bytes) = store.meta_get(IDENTITY_MODE_KEY)?
        && let Some(mode) = decode_identity_mode(&bytes)
    {
        return Ok(mode);
    }
    let mode = if any_document_is_tagged(root)? {
        IdentityMode::Tagged
    } else {
        default
    };
    store.meta_set(IDENTITY_MODE_KEY, &encode_identity_mode(mode))?;
    Ok(mode)
}

fn encode_identity_mode(mode: IdentityMode) -> [u8; 1] {
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

/// Whether any already-discoverable document in `root` carries at least one `id:` tag — decides a
/// brand-new workspace's identity mode (never re-checked once minted).
fn any_document_is_tagged(root: &Path) -> Result<bool, WorkspaceError> {
    for rel in walker::walk(root)? {
        if walker::is_notes_document(basename(&rel)) {
            continue;
        }
        let Ok(bytes) = std::fs::read(root.join(rel.as_str())) else {
            continue;
        };
        let file = txtodo_core::parse_file(&bytes);
        if file
            .lines
            .iter()
            .any(|l| crate::fastid::fast_id_of(l).is_some())
        {
            return Ok(true);
        }
    }
    Ok(false)
}
