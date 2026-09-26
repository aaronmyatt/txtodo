//! Unit tests for `walk_scope.rs`: which workspace roots share lists.

use super::*;
use std::path::PathBuf;

/// A fresh directory tree under the OS temp dir (this crate has no dependencies, so no
/// `tempfile`), canonical so the paths look like registered roots. `mk` lists directories to
/// create, relative to the returned root.
fn tree(name: &str, mk: &[&str]) -> PathBuf {
    let root = std::env::temp_dir().join(format!("txtodo-ws-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("create test root");
    for d in mk {
        std::fs::create_dir_all(root.join(d)).expect("create test dir");
    }
    root.canonicalize().expect("canonical test root")
}

#[test]
fn a_folder_below_a_workspace_is_inside_it_and_the_workspace_is_around_it() {
    let root = tree("nest", &["tasks/slug/deeper"]);
    let slug = root.join("tasks/slug");
    assert_eq!(root_overlap(&slug, &root), Some(RootOverlap::Inside));
    assert_eq!(root_overlap(&root, &slug), Some(RootOverlap::Around));
    assert_eq!(
        root_overlap(&root.join("tasks/slug/deeper"), &root),
        Some(RootOverlap::Inside)
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn the_same_folder_a_sibling_and_a_name_prefix_do_not_overlap() {
    let root = tree("apart", &["a", "b", "ab"]);
    let a = root.join("a");
    assert_eq!(
        root_overlap(&a, &a),
        None,
        "an exact root is deduped, not refused"
    );
    assert_eq!(root_overlap(&a, &root.join("b")), None);
    assert_eq!(
        root_overlap(&root.join("ab"), &a),
        None,
        "by component, not by text"
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// A linked worktree carries `.git` (a file there, a dir here: both count). The walk never enters
/// it, so it is a workspace of its own, not a folder of the clone around it.
#[test]
fn a_nested_git_checkout_shares_nothing_with_the_workspace_around_it() {
    let root = tree("checkout", &["wt/.git", "wt/sub"]);
    let wt = root.join("wt");
    assert_eq!(root_overlap(&wt, &root), None);
    assert_eq!(root_overlap(&root, &wt), None);
    assert_eq!(
        root_overlap(&wt.join("sub"), &root),
        None,
        "the checkout is on the way"
    );
    assert_eq!(
        root_overlap(&wt.join("sub"), &wt),
        Some(RootOverlap::Inside),
        "inside the checkout itself it still overlaps"
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// A `--dir` daemon mirrors offered workspaces under `<dir>/.txtodo/remote/`; `node_modules` and
/// `.claude/worktrees` are skipped by name. None of them is walked.
#[test]
fn state_and_skipped_folders_are_never_walked() {
    let root = tree(
        "skipped",
        &[
            ".txtodo/remote/x",
            "node_modules/pkg",
            ".claude/worktrees/w",
        ],
    );
    for inner in [
        ".txtodo/remote/x",
        "node_modules/pkg",
        ".claude/worktrees/w",
    ] {
        assert_eq!(root_overlap(&root.join(inner), &root), None, "{inner}");
        assert!(!walks_into(&root, &root.join(inner)), "{inner}");
    }
    assert!(
        walks_into(&root, &root.join(".claude")),
        "a hidden folder is walked"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn skipped_dirs_match_the_walker_rule() {
    let root = tree(
        "rule",
        &[
            "node_modules",
            "clone/.git",
            "built/target",
            "tasks/target",
            ".claude/worktrees",
        ],
    );
    std::fs::write(root.join("built/Cargo.toml"), "").expect("write Cargo.toml");
    assert!(is_skipped_dir(&root.join("node_modules")));
    assert!(is_skipped_dir(&root.join("clone")));
    assert!(is_skipped_dir(&root.join("built/target")));
    assert!(
        !is_skipped_dir(&root.join("tasks/target")),
        "a ref dir called target"
    );
    assert!(is_skipped_dir(&root.join(".claude/worktrees")));
    assert!(!is_skipped_dir(&root.join("tasks")));
    let _ = std::fs::remove_dir_all(&root);
}
