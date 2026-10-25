//! `repo_scan.rs` on real temp trees.

use super::*;

fn mk(root: &Path, rel: &str) {
    std::fs::create_dir_all(root.join(rel)).unwrap();
}

fn put(root: &Path, rel: &str) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, "x\n").unwrap();
}

/// A clone at `rel` under `root`: a `.git` folder and the named file.
fn clone_with(root: &Path, rel: &str, list: &str) {
    mk(root, &format!("{rel}/.git"));
    put(root, &format!("{rel}/{list}"));
}

fn found(root: &Path, skip: &[PathBuf]) -> Vec<String> {
    let scanned = scan(&[root.to_path_buf()], skip);
    scanned
        .repos
        .iter()
        .map(|p| p.strip_prefix(root).unwrap().to_string_lossy().into_owned())
        .collect()
}

#[test]
fn a_clone_with_a_todo_txt_or_a_txtodo_toml_is_found_and_one_without_a_list_is_not() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    clone_with(root, "code/a", "todo.txt");
    clone_with(root, "code/b", "txtodo.toml");
    mk(root, "code/c/.git"); // a clone with no list
    put(root, "code/d/todo.txt"); // a list with no clone
    assert_eq!(found(root, &[]), ["code/a", "code/b"]);
}

#[test]
fn a_nested_clone_is_found_beside_the_one_around_it() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    clone_with(root, "outer", "todo.txt");
    clone_with(root, "outer/vendored-by-hand/inner", "todo.txt");
    assert_eq!(found(root, &[]), ["outer", "outer/vendored-by-hand/inner"]);
}

#[test]
fn hidden_build_and_vendor_folders_and_library_are_not_entered() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    for skipped in [
        ".claude/worktrees/x",
        ".cache/y",
        "node_modules/z",
        "app/target/q",
        "app/vendor/v",
        "app/dist/d",
        "Library/app",
    ] {
        clone_with(root, skipped, "todo.txt");
    }
    clone_with(root, "app/Library/deep", "todo.txt"); // only a top-level `Library` is skipped
    assert_eq!(found(root, &[]), ["app/Library/deep"]);
}

#[test]
fn a_git_file_is_a_worktree_or_submodule_and_is_left_to_a_first_touch() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    mk(root, "wt");
    put(root, "wt/.git"); // a file, as a linked worktree has
    put(root, "wt/todo.txt");
    assert!(found(root, &[]).is_empty());
}

#[test]
fn the_walk_stops_at_the_depth_limit() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let at = |n: usize| {
        (0..n)
            .map(|i| format!("d{i}"))
            .collect::<Vec<_>>()
            .join("/")
    };
    clone_with(root, &at(MAX_DEPTH), "todo.txt");
    clone_with(root, &format!("x/{}", at(MAX_DEPTH)), "todo.txt"); // one level too deep
    let got = found(root, &[]);
    assert_eq!(got, [at(MAX_DEPTH)], "{got:?}");
}

#[test]
fn a_folder_in_the_skip_list_is_not_entered() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    clone_with(root, "txtodo/lists/work", "todo.txt");
    clone_with(root, "code/a", "todo.txt");
    let data = root.join("txtodo");
    assert_eq!(found(root, &[data]), ["code/a"]);
}

#[cfg(unix)]
#[test]
fn a_symbolic_link_is_never_followed_so_a_loop_ends() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    clone_with(root, "real", "todo.txt");
    std::os::unix::fs::symlink(root, root.join("loop")).unwrap();
    std::os::unix::fs::symlink(root.join("real"), root.join("alias")).unwrap();
    assert_eq!(found(root, &[]), ["real"]);
}

#[test]
fn several_roots_are_walked_and_a_repo_met_twice_counts_once() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    clone_with(root, "a", "todo.txt");
    clone_with(root, "b", "todo.txt");
    let scanned = scan(
        &[root.to_path_buf(), root.join("a"), root.join("gone")],
        &[],
    );
    assert_eq!(scanned.repos.len(), 2, "{:?}", scanned.repos);
    assert!(!scanned.truncated);
}

#[test]
fn a_scan_is_repeatable_in_name_order() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    for name in ["m", "b", "z", "a"] {
        clone_with(root, name, "todo.txt");
    }
    assert_eq!(found(root, &[]), ["a", "b", "m", "z"]);
}
