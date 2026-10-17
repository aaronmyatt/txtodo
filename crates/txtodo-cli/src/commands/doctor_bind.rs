//! `txtodo doctor`'s `bind` rows (task spike-txtodo-should-merge-remote-workspa): a workspace of
//! this device's own that shares its name with a Remote one. That is how a repo cloned on two
//! devices looks here: the clone is its own workspace, and the other device's list of the same
//! project arrives beside it as a mirror, so an agent in the clone reads the wrong list. A name is
//! a weak match (two unrelated `notes` folders), so this only warns and names the fix; nothing
//! binds on its own.

use super::doctor::{Check, Status, check};
use txtodo_proto::v1 as pb;

/// One WARN row per (own workspace, Remote workspace) pair with the same shown name, ignoring case
/// and surrounding spaces. The default workspace is never one of them: it merges by its reserved
/// id instead (ADR 0029).
pub(super) fn bind_checks(workspaces: &[pb::WorkspaceInfo]) -> Vec<Check> {
    let key = |w: &pb::WorkspaceInfo| w.name.trim().to_lowercase();
    let own = workspaces.iter().filter(|w| !w.is_remote && !w.is_default);
    let mut rows = Vec::new();
    for mine in own.filter(|w| !key(w).is_empty()) {
        let same = workspaces
            .iter()
            .filter(|w| w.is_remote && key(w) == key(mine));
        rows.extend(same.map(|remote| bind_row(mine, remote)));
    }
    rows
}

fn bind_row(mine: &pb::WorkspaceInfo, remote: &pb::WorkspaceInfo) -> Check {
    check(
        "bind",
        Status::Warn,
        format!(
            "{} ({}) and the Remote workspace {} share the name {:?}: if they are one project, \
             this folder is not reading the synced list. Use it here: `txtodo workspace bind {} \
             {}` (this folder's own list moves aside, nothing is deleted)",
            mine.root,
            mine.workspace_id,
            remote.workspace_id,
            mine.name,
            remote.workspace_id,
            mine.root
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(id: &str, name: &str, remote: bool, default: bool) -> pb::WorkspaceInfo {
        pb::WorkspaceInfo {
            workspace_id: id.to_owned(),
            root: format!("/home/a/{id}"),
            name: name.to_owned(),
            is_remote: remote,
            is_default: default,
            ..pb::WorkspaceInfo::default()
        }
    }

    #[test]
    fn a_clone_beside_a_mirror_of_the_same_name_is_one_warn_naming_the_bind_command() {
        let rows = bind_checks(&[
            info("CLONE", "txtodo", false, false),
            info("MIRROR", "TxTodo ", true, false),
            info("OTHER", "givens", true, false),
        ]);
        assert_eq!(rows.len(), 1, "{rows:?}");
        assert_eq!((rows[0].name, rows[0].status), ("bind", Status::Warn));
        let detail = &rows[0].detail;
        assert!(
            detail.contains("`txtodo workspace bind MIRROR /home/a/CLONE`"),
            "{detail}"
        );
        assert!(detail.contains("/home/a/CLONE (CLONE)"), "{detail}");
    }

    #[test]
    fn no_row_for_the_default_two_mirrors_or_an_empty_name() {
        let rows = bind_checks(&[
            info("DEFAULT", "default", false, true),
            info("ALIAS", "default", true, false),
            info("M1", "notes", true, false),
            info("M2", "notes", true, false),
            info("NAMELESS", "", false, false),
            info("M3", "", true, false),
        ]);
        assert!(rows.is_empty(), "{rows:?}");
    }
}
