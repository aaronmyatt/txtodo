//! `txtodo doctor`'s `overlap` rows (sync-drift line 3): registered workspaces whose lists
//! overlap, one inside the other. The daemon now refuses a new one, but a registry from before
//! that keeps loading them, and two stores then track one file with their own ids, so lines come
//! back doubled or move to the bottom. Report only: nothing is unregistered or deleted here.

use super::doctor::{Check, Status, check};
use crate::client;
use std::path::Path;
use txtodo_proto::v1 as pb;
use txtodo_workspace_paths::{RootOverlap, root_overlap};

/// Every registered workspace, or none: a `--dir` bridge daemon has no registry
/// (`Unimplemented`), and with no daemon there is nothing to list.
pub(super) fn registered_workspaces(daemon: Option<&mut client::Daemon>) -> Vec<pb::WorkspaceInfo> {
    daemon
        .and_then(|d| d.workspace_list().ok())
        .unwrap_or_default()
}

/// One FAIL row per pair of registered workspaces, both still on disk, where the walk of one
/// reaches the other's lists (`txtodo_workspace_paths::root_overlap`, the rule the daemon refuses
/// a new root by).
pub(super) fn overlap_checks(workspaces: &[pb::WorkspaceInfo]) -> Vec<Check> {
    let live: Vec<&pb::WorkspaceInfo> = workspaces.iter().filter(|w| w.root_exists).collect();
    let mut rows = Vec::new();
    for (i, a) in live.iter().enumerate() {
        for b in live.iter().skip(i + 1) {
            match root_overlap(Path::new(&a.root), Path::new(&b.root)) {
                Some(RootOverlap::Inside) => rows.push(overlap_row(a, b)),
                Some(RootOverlap::Around) => rows.push(overlap_row(b, a)),
                None => {}
            }
        }
    }
    rows
}

/// `inner`'s lists are also `outer`'s. The one to remove is the inner one, unless that is the
/// default workspace, which cannot be removed.
fn overlap_row(inner: &pb::WorkspaceInfo, outer: &pb::WorkspaceInfo) -> Check {
    let remove = if inner.is_default { outer } else { inner };
    check(
        "overlap",
        Status::Fail,
        format!(
            "{} ({}) is inside the workspace {} ({}): two stores track its lists, so lines \
             double or move; remove one: `txtodo workspace remove {}`",
            inner.root, inner.workspace_id, outer.root, outer.workspace_id, remove.workspace_id
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(id: &str, root: &Path, is_default: bool) -> pb::WorkspaceInfo {
        pb::WorkspaceInfo {
            workspace_id: id.to_owned(),
            root: root.display().to_string(),
            root_exists: root.exists(),
            is_default,
            ..pb::WorkspaceInfo::default()
        }
    }

    #[test]
    fn a_workspace_inside_another_is_one_fail_row_naming_both_and_the_one_to_remove() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        std::fs::create_dir_all(root.join("tasks/slug")).unwrap();
        std::fs::create_dir_all(root.join("wt/.git")).unwrap();
        let rows = overlap_checks(&[
            info("OUTER", &root, false),
            info("INNER", &root.join("tasks/slug"), false),
            info("WORKTREE", &root.join("wt"), false),
        ]);
        assert_eq!(
            rows.len(),
            1,
            "a nested git checkout is not an overlap: {rows:?}"
        );
        assert_eq!((rows[0].name, rows[0].status), ("overlap", Status::Fail));
        let detail = &rows[0].detail;
        assert!(
            detail.contains("(INNER) is inside the workspace"),
            "{detail}"
        );
        assert!(detail.contains("(OUTER)"), "{detail}");
        assert!(
            detail.ends_with("`txtodo workspace remove INNER`"),
            "{detail}"
        );
    }

    #[test]
    fn the_default_inside_another_workspace_points_at_the_outer_one_to_remove() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        std::fs::create_dir_all(root.join("share/default")).unwrap();
        // Listed default first, the other way round from the test above.
        let rows = overlap_checks(&[
            info("DEFAULT", &root.join("share/default"), true),
            info("HOME", &root, false),
        ]);
        assert_eq!(rows.len(), 1);
        assert!(rows[0].detail.contains("(DEFAULT) is inside"), "{rows:?}");
        assert!(
            rows[0].detail.ends_with("`txtodo workspace remove HOME`"),
            "{rows:?}"
        );
    }

    #[test]
    fn a_missing_root_or_an_empty_registry_reports_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let gone = info("GONE", &root.join("tasks/gone"), false);
        assert!(!gone.root_exists);
        assert!(overlap_checks(&[info("OUTER", &root, false), gone]).is_empty());
        assert!(overlap_checks(&[]).is_empty());
        assert!(registered_workspaces(None).is_empty());
    }
}
