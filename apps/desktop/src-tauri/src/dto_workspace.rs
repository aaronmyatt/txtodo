//! `WorkspaceInfo` DTO (ADR 0025, task `desktop-workspace-switcher`), split out of `dto.rs` the
//! same way `dto_notes.rs`/`dto_tokens.rs`/`dto_pairing.rs`/`dto_activity.rs` already are.

use serde::Serialize;
use txtodo_proto::v1 as pb;

/// One entry in the device-global workspace registry (`WorkspaceList`).
#[derive(Debug, Clone, Serialize)]
pub struct WorkspaceInfoDto {
    /// ULID text.
    pub id: String,
    /// Canonicalized absolute path.
    pub root: String,
    /// Unix ms this workspace was first registered.
    pub added_at_ms: u64,
    /// Whether `root` still exists on disk.
    pub root_exists: bool,
    /// Whether `root/.txtodo/oplog.db` exists — false means never opened yet.
    pub has_state: bool,
    /// How far the daemon is through opening this workspace (task `daemon-early-bind`): `"queued"`,
    /// `"loading"`, `"ready"`, `"failed"`, or `"unknown"` from a daemon that predates the field.
    pub load_state: &'static str,
    /// Why the last open failed; empty unless `load_state` is `"failed"`.
    pub load_error: String,
    /// True for the user's default workspace (task default-workspace): shown as "Default" and
    /// never removable.
    pub is_default: bool,
}

/// Whether a fan-out (`universal_tasks`, `op_log_all`) may query `w` now. A workspace the daemon
/// has not finished opening is skipped rather than promoted: asking for it would queue-jump every
/// workspace the daemon is still opening in its own most-recently-used order. `Unspecified` (an
/// older daemon, or a root it never scheduled) is queried as before.
pub fn is_ready_or_unknown(w: &pb::WorkspaceInfo) -> bool {
    matches!(
        pb::WorkspaceLoadState::try_from(w.load_state),
        Ok(pb::WorkspaceLoadState::Ready | pb::WorkspaceLoadState::Unspecified) | Err(_)
    )
}

fn load_state_name(raw: i32) -> &'static str {
    match pb::WorkspaceLoadState::try_from(raw) {
        Ok(pb::WorkspaceLoadState::Queued) => "queued",
        Ok(pb::WorkspaceLoadState::Loading) => "loading",
        Ok(pb::WorkspaceLoadState::Ready) => "ready",
        Ok(pb::WorkspaceLoadState::Failed) => "failed",
        Ok(pb::WorkspaceLoadState::Unspecified) | Err(_) => "unknown",
    }
}

impl From<pb::WorkspaceInfo> for WorkspaceInfoDto {
    fn from(w: pb::WorkspaceInfo) -> WorkspaceInfoDto {
        WorkspaceInfoDto {
            id: w.workspace_id,
            root: w.root,
            added_at_ms: w.added_at_ms,
            root_exists: w.root_exists,
            has_state: w.has_state,
            load_state: load_state_name(w.load_state),
            load_error: w.load_error,
            is_default: w.is_default,
        }
    }
}
