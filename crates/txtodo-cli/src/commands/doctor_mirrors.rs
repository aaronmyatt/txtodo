//! `txtodo doctor`'s `mirror` rows (task `two-kinds-of-workspace`, ADR 0043): one per Remote mirror,
//! the folders an older build made at `<data dir>/remote/<id>/` for a workspace a paired device
//! offered. A mirror no longer syncs and a repo travels with git, so each is to be moved aside
//! (`txtodo workspace prune --repo-mirrors`, never deleting). But a mirror can hold an edit that
//! only ever reached this device that way, so the row says how many of its lines the same-named
//! clone does not have, and the first one. It replaces the `bind` rows, which pointed at a command
//! that is going.
//!
//! Only the root `todo.txt` of each is compared, by exact line text; a sub-list is not.

use std::path::Path;

use super::doctor::{Check, Status, check};
use txtodo_proto::v1 as pb;

/// The lines of `path`'s `todo.txt`, blank ones left out; empty when it cannot be read.
fn lines_of(root: &str) -> Vec<String> {
    let text = std::fs::read_to_string(Path::new(root).join("todo.txt")).unwrap_or_default();
    text.lines()
        .map(str::trim_end)
        .filter(|l| !l.is_empty())
        .map(str::to_owned)
        .collect()
}

/// A name compared as `doctor`'s old `bind` rows did: ignoring case and surrounding spaces.
fn key(w: &pb::WorkspaceInfo) -> String {
    w.name.trim().to_lowercase()
}

/// One row per Remote mirror whose folder still exists, against the repo workspace of the same
/// name when there is one.
pub(super) fn mirror_checks(workspaces: &[pb::WorkspaceInfo]) -> Vec<Check> {
    workspaces
        .iter()
        .filter(|w| w.is_remote && w.root_exists)
        .map(|mirror| {
            let clone = workspaces.iter().find(|w| {
                !w.is_remote && !w.is_default && !key(w).is_empty() && key(w) == key(mirror)
            });
            mirror_row(mirror, clone)
        })
        .collect()
}

fn mirror_row(mirror: &pb::WorkspaceInfo, clone: Option<&pb::WorkspaceInfo>) -> Check {
    let have: std::collections::HashSet<String> = clone
        .map(|c| lines_of(&c.root).into_iter().collect())
        .unwrap_or_default();
    let lacking: Vec<String> = lines_of(&mirror.root)
        .into_iter()
        .filter(|l| !have.contains(l))
        .collect();
    let fix = "`txtodo workspace prune --repo-mirrors` moves mirrors aside, deleting nothing";
    let against = clone.map_or_else(
        || "no repo workspace of the same name".to_owned(),
        |c| c.root.clone(),
    );
    match lacking.first() {
        None => check(
            "mirror",
            Status::Ok,
            format!("{} has no line that {against} lacks; {fix}", mirror.root),
        ),
        Some(first) => check(
            "mirror",
            Status::Warn,
            format!(
                "{} holds {} line(s) that {against} lacks, the first {first:?}: copy what you \
                 want to keep into your list, then {fix}",
                mirror.root,
                lacking.len()
            ),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(dir: &Path, name: &str, text: &str) -> String {
        let root = dir.join(name);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("todo.txt"), text).unwrap();
        root.display().to_string()
    }

    fn info(root: String, name: &str, remote: bool) -> pb::WorkspaceInfo {
        pb::WorkspaceInfo {
            workspace_id: name.to_owned(),
            root,
            name: name.to_owned(),
            is_remote: remote,
            root_exists: true,
            ..pb::WorkspaceInfo::default()
        }
    }

    #[test]
    fn a_mirror_with_a_line_its_clone_lacks_is_a_warn_naming_the_line_and_the_fix() {
        let tmp = tempfile::tempdir().unwrap();
        let mirror = folder(tmp.path(), "m", "(A) only here\n\n(B) both\n");
        let clone = folder(tmp.path(), "c", "(B) both\n");
        let rows = mirror_checks(&[
            info(clone, "proj", false),
            info(mirror.clone(), " Proj", true),
        ]);
        assert_eq!(rows.len(), 1, "{rows:?}");
        assert_eq!((rows[0].name, rows[0].status), ("mirror", Status::Warn));
        let detail = &rows[0].detail;
        assert!(
            detail.contains("1 line(s)") && detail.contains("only here"),
            "{detail}"
        );
        assert!(
            detail.contains("workspace prune --repo-mirrors"),
            "{detail}"
        );
        assert!(detail.contains(&mirror), "{detail}");
    }

    #[test]
    fn a_mirror_the_clone_covers_is_ok_and_one_with_no_clone_warns_about_every_line() {
        let tmp = tempfile::tempdir().unwrap();
        let covered = folder(tmp.path(), "m1", "(B) both\n");
        let clone = folder(tmp.path(), "c", "(B) both\n(C) more\n");
        let alone = folder(tmp.path(), "m2", "(A) a\n(B) b\n");
        let rows = mirror_checks(&[
            info(clone, "proj", false),
            info(covered, "proj", true),
            info(alone, "other", true),
        ]);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].status, Status::Ok, "{rows:?}");
        assert_eq!(rows[1].status, Status::Warn, "{rows:?}");
        assert!(rows[1].detail.contains("2 line(s)"), "{:?}", rows[1]);
        assert!(
            rows[1]
                .detail
                .contains("no repo workspace of the same name"),
            "{:?}",
            rows[1]
        );
    }

    #[test]
    fn nothing_for_a_workspace_that_is_not_a_mirror_or_a_mirror_whose_folder_is_gone() {
        let tmp = tempfile::tempdir().unwrap();
        let mut gone = info(folder(tmp.path(), "g", "(A) x\n"), "g", true);
        gone.root_exists = false;
        let plain = info(folder(tmp.path(), "p", "(A) y\n"), "p", false);
        assert!(mirror_checks(&[gone, plain]).is_empty());
    }
}
