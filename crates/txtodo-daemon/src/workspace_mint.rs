//! Loads-or-mints every workspace-lifetime id that lives in `meta` (device, group, identity mode).
//! Split out of `workspace.rs` to keep that file within its line budget.

use crate::clock::Clock;
use crate::walker;
use crate::workspace_error::WorkspaceError;
use std::path::Path;
use txtodo_model::{DeviceId, FilePath, IdentityMode, Ulid};
use txtodo_store::{Store, StoreError};
use txtodo_sync::GroupId;

/// The `meta` key holding this install's device id.
pub(crate) const DEVICE_ID_KEY: &str = "device_id";
/// The `meta` key holding this workspace's sync group id (plan M4 pairing).
pub(crate) const GROUP_ID_KEY: &str = "group_id";
/// The `meta` key holding this workspace's identity mode (docs/questions.md Q2).
pub(crate) const IDENTITY_MODE_KEY: &str = "identity_mode";

/// The last `/`-separated segment of a workspace-relative path.
pub(crate) fn basename(path: &FilePath) -> &str {
    path.as_str().rsplit('/').next().unwrap_or(path.as_str())
}

pub(crate) fn load_or_mint_device(
    store: &mut Store,
    clock: &dyn Clock,
) -> Result<DeviceId, StoreError> {
    if let Some(bytes) = store.meta_get(DEVICE_ID_KEY)?
        && let Ok(raw) = <[u8; 16]>::try_from(bytes.as_slice())
    {
        return Ok(DeviceId::new(Ulid::from_u128(u128::from_be_bytes(raw))));
    }
    let id = DeviceId::new(clock.new_ulid());
    store.meta_set(DEVICE_ID_KEY, &id.ulid().to_u128().to_be_bytes())?;
    debug_assert!(store.meta_get(DEVICE_ID_KEY)?.is_some());
    Ok(id)
}

/// Loads this workspace's sync group id from `meta`, or mints a fresh one (128 random bits; unlike
/// the device id, nothing needs to sort on it) and persists it. `getrandom` failure is not
/// recoverable in a meaningful way — `clock.rs`'s `SystemClock::new_ulid` takes the same stance for
/// a ULID's random half — so a fixed fallback pattern still yields a usable, if less unique, id.
pub(crate) fn load_or_mint_group(store: &mut Store) -> Result<GroupId, StoreError> {
    if let Some(bytes) = store.meta_get(GROUP_ID_KEY)?
        && let Ok(raw) = <[u8; 16]>::try_from(bytes.as_slice())
    {
        return Ok(GroupId(u128::from_be_bytes(raw)));
    }
    let mut raw = [0u8; 16];
    if getrandom::fill(&mut raw).is_err() {
        raw = [0xA5; 16];
    }
    store.meta_set(GROUP_ID_KEY, &raw)?;
    Ok(GroupId(u128::from_be_bytes(raw)))
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
