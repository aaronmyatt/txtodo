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
}

impl From<pb::WorkspaceInfo> for WorkspaceInfoDto {
    fn from(w: pb::WorkspaceInfo) -> WorkspaceInfoDto {
        WorkspaceInfoDto {
            id: w.workspace_id,
            root: w.root,
            added_at_ms: w.added_at_ms,
            root_exists: w.root_exists,
            has_state: w.has_state,
        }
    }
}
