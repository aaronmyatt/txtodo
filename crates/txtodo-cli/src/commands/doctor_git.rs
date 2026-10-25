//! `txtodo doctor`'s `git` rows (task `two-kinds-of-workspace`, ADR 0043): a repo workspace, a git
//! clone whose todo lists git carries, that has no merge driver set up here. Without it git's own
//! merge can stop on a conflict between two edits of one list; with it the lists merge line by line
//! and no line is lost. The driver is set per clone (git never shares config), so every clone on
//! every machine needs `txtodo git setup` once. A WARN, never a FAIL: a clone without it still
//! works, and one set up gets no row.

use std::path::Path;

use super::doctor::{Check, Status, check};
use super::git_setup::{Plan, plan_for};
use txtodo_proto::v1 as pb;

/// One WARN row per repo workspace that is a git clone and still needs `txtodo git setup`. Mirrors
/// of a paired device's workspace (retired at the cut) and clones whose state cannot be read (no
/// `git` on `PATH`) get none.
pub(super) fn git_checks(workspaces: &[pb::WorkspaceInfo]) -> Vec<Check> {
    workspaces
        .iter()
        .filter(|w| w.kind() == pb::WorkspaceKind::Repo && !w.is_remote && w.root_exists)
        .filter(|w| Path::new(&w.root).join(".git").exists())
        .filter_map(|w| plan_for(Path::new(&w.root)).ok().map(|(plan, _)| (w, plan)))
        .filter(|(_, plan)| !plan.is_empty())
        .map(|(w, plan)| git_row(&w.root, &plan))
        .collect()
}

fn git_row(root: &str, plan: &Plan) -> Check {
    let mut missing = Vec::new();
    if plan.driver.is_some() {
        missing.push("the merge driver in this clone's git config");
    }
    if !plan.attributes.is_empty() {
        missing.push("lines in .gitattributes");
    }
    if plan.exclude {
        missing.push(".txtodo/ in .git/info/exclude");
    }
    check(
        "git",
        Status::Warn,
        format!(
            "{root} is a git clone without {}: two edits of one list can stop git on a conflict. \
             Fix: `txtodo git setup --dir {root}` (then commit .gitattributes)",
            missing.join(", ")
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn info(root: &Path, kind: pb::WorkspaceKind, remote: bool) -> pb::WorkspaceInfo {
        pb::WorkspaceInfo {
            workspace_id: "01W".to_owned(),
            root: root.display().to_string(),
            root_exists: true,
            is_remote: remote,
            kind: kind as i32,
            ..pb::WorkspaceInfo::default()
        }
    }

    fn clone() -> Option<tempfile::TempDir> {
        let dir = tempfile::tempdir().unwrap();
        let init = Command::new("git")
            .args(["init", "-q"])
            .current_dir(dir.path())
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_INDEX_FILE")
            .output()
            .ok()?;
        init.status.success().then_some(dir)
    }

    #[test]
    fn a_repo_clone_with_no_driver_gets_a_warn_naming_the_fix_and_setup_clears_it() {
        let Some(dir) = clone() else {
            return;
        };
        let repo = info(dir.path(), pb::WorkspaceKind::Repo, false);
        let rows = git_checks(std::slice::from_ref(&repo));
        assert_eq!(rows.len(), 1, "{rows:?}");
        assert_eq!((rows[0].name, rows[0].status), ("git", Status::Warn));
        let why = &rows[0].detail;
        assert!(why.contains("txtodo git setup --dir"), "{why}");
        assert!(why.contains("merge driver"), "{why}");

        let dir_arg = dir.path().to_str();
        super::super::git_setup::run(
            &super::super::git_setup::Action::Setup { dry_run: false },
            dir_arg,
        )
        .unwrap();
        assert!(git_checks(&[repo]).is_empty(), "set up: no row");
    }

    #[test]
    fn lists_mirrors_and_plain_folders_get_no_row() {
        let Some(dir) = clone() else {
            return;
        };
        assert!(git_checks(&[info(dir.path(), pb::WorkspaceKind::List, false)]).is_empty());
        assert!(git_checks(&[info(dir.path(), pb::WorkspaceKind::Repo, true)]).is_empty());
        let plain = tempfile::tempdir().unwrap();
        assert!(git_checks(&[info(plain.path(), pb::WorkspaceKind::Repo, false)]).is_empty());
    }
}
