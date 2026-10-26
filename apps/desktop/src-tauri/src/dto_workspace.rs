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
    /// True for a mirror of a paired device's workspace (task remote-workspace-mirror): the daemon
    /// chose its folder, so the switcher labels it "Remote".
    pub is_remote: bool,
    /// The name every paired device shows for it (task workspace-vanity-name): its `txtodo.toml`
    /// name, else `default` or the folder's, else for a mirror what its offering device calls it.
    /// Empty from an older daemon.
    pub name: String,
    /// Which carrier syncs it (task two-kinds-of-workspace, ADR 0043): `"list"` (txtodo syncs it
    /// between the user's own devices; the default is one), `"repo"` (git does), or `""` from a
    /// daemon that predates the field.
    pub kind: &'static str,
}

/// Where a workspace keeps its root list and the folder for its `ref:` lines (task
/// workspace-layout); `WorkspaceLayoutInfo` on the wire.
#[derive(Debug, Clone, Serialize)]
pub struct WorkspaceLayoutDto {
    /// Relative to the workspace root; `"."` means beside the list.
    pub refs_dir: String,
    /// The root list's path.
    pub todo_file: String,
    /// Why `txtodo.toml` is not the layout in force; empty when it is.
    pub note: String,
    /// Folders holding a todo.txt or notes.md that sit outside `refs_dir` and nothing points at.
    pub outside_refs_dir: Vec<String>,
}

impl From<pb::WorkspaceLayoutInfo> for WorkspaceLayoutDto {
    fn from(l: pb::WorkspaceLayoutInfo) -> WorkspaceLayoutDto {
        WorkspaceLayoutDto {
            refs_dir: l.refs_dir,
            todo_file: l.todo_file,
            note: l.note,
            outside_refs_dir: l.outside_refs_dir,
        }
    }
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

fn kind_name(raw: i32) -> &'static str {
    match pb::WorkspaceKind::try_from(raw) {
        Ok(pb::WorkspaceKind::List) => "list",
        Ok(pb::WorkspaceKind::Repo) => "repo",
        Ok(pb::WorkspaceKind::Unspecified) | Err(_) => "",
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
            is_remote: w.is_remote,
            name: w.name,
            kind: kind_name(w.kind),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Task remote-workspace-mirror: the switcher's "Remote" label reads this flag.
    #[test]
    fn the_remote_flag_reaches_the_dto() {
        let w = pb::WorkspaceInfo {
            workspace_id: "01BX5ZZKBKACTAV9WEVGEMMVRZ".into(),
            is_remote: true,
            ..pb::WorkspaceInfo::default()
        };
        assert!(WorkspaceInfoDto::from(w).is_remote);
        assert!(!WorkspaceInfoDto::from(pb::WorkspaceInfo::default()).is_remote);
    }

    /// Task two-kinds-of-workspace: the switcher groups by this, and an older daemon says nothing.
    #[test]
    fn the_kind_reaches_the_dto_as_list_repo_or_nothing() {
        let kind = |k: pb::WorkspaceKind| {
            let w = pb::WorkspaceInfo {
                kind: k as i32,
                ..pb::WorkspaceInfo::default()
            };
            WorkspaceInfoDto::from(w).kind
        };
        assert_eq!(kind(pb::WorkspaceKind::List), "list");
        assert_eq!(kind(pb::WorkspaceKind::Repo), "repo");
        assert_eq!(kind(pb::WorkspaceKind::Unspecified), "");
    }

    /// Task workspace-vanity-name: the switcher shows this name for a mirror.
    #[test]
    fn the_shown_name_reaches_the_dto() {
        let w = pb::WorkspaceInfo {
            name: "House plants".into(),
            ..pb::WorkspaceInfo::default()
        };
        assert_eq!(WorkspaceInfoDto::from(w).name, "House plants");
        assert!(
            WorkspaceInfoDto::from(pb::WorkspaceInfo::default())
                .name
                .is_empty()
        );
    }
}
