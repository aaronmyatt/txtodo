//! `dir_bridge_guard::global_owner` (task mcp-dir-second-daemon): a `--dir` bridge is refused on a
//! root the global registry holds, a folder inside it, or a folder around it; allowed anywhere
//! else, and whenever the bridge shares that registry or no global registry exists. Hermetic: the
//! global registry is built under a temp `$XDG_DATA_HOME`, never the machine's own.

use crate::clock::FakeClock;
use crate::dir_bridge_guard::global_owner;
use crate::workspace_registry::WorkspaceRegistry;
use crate::workspace_registry_paths::{RegistryEnv, registry_db_path};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn env(data: &Path, extra: &[(&str, &Path)]) -> RegistryEnv {
    let mut vars = BTreeMap::from([("XDG_DATA_HOME".to_owned(), data.display().to_string())]);
    for (k, v) in extra {
        vars.insert((*k).to_owned(), v.display().to_string());
    }
    RegistryEnv::new(vars, data.to_path_buf())
}

/// A temp tree with `ws/` registered in the global registry under `data/`.
fn registered() -> (tempfile::TempDir, PathBuf, RegistryEnv) {
    let tmp = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    let base = tmp.path().canonicalize().unwrap_or_else(|e| panic!("{e}"));
    let root = base.join("ws");
    std::fs::create_dir_all(root.join("tasks/sub")).unwrap_or_else(|e| panic!("{e}"));
    let env = env(&base.join("data"), &[]);
    let db = registry_db_path(&env);
    std::fs::create_dir_all(db.parent().unwrap_or(&base)).unwrap_or_else(|e| panic!("{e}"));
    let mut registry = WorkspaceRegistry::open(&db).unwrap_or_else(|e| panic!("{e}"));
    registry
        .add(&root, &FakeClock::new(1_000))
        .unwrap_or_else(|e| panic!("{e}"));
    (tmp, root, env)
}

#[test]
fn the_root_a_folder_inside_it_and_one_around_it_are_refused() {
    let (_tmp, root, env) = registered();
    for dir in [
        root.clone(),
        root.join("tasks/sub"),
        root.parent().map(Path::to_path_buf).unwrap_or_default(),
    ] {
        let owner = global_owner(&env, &dir);
        assert_eq!(
            owner.map(|(_, r)| r),
            Some(root.clone()),
            "--dir {dir:?} overlaps the registered root"
        );
    }
}

#[test]
fn an_unrelated_folder_is_allowed() {
    let (tmp, _root, env) = registered();
    let elsewhere = tmp.path().join("elsewhere");
    std::fs::create_dir_all(&elsewhere).unwrap_or_else(|e| panic!("{e}"));
    let elsewhere = elsewhere.canonicalize().unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(global_owner(&env, &elsewhere), None);
}

#[test]
fn a_bridge_sharing_the_registry_or_with_none_to_read_is_allowed() {
    let (tmp, root, env) = registered();
    // `$TXTODO_REGISTRY_DB` names one file for the bridge and the "global" lookup: it is its own.
    let db = registry_db_path(&env);
    let shared = self::env(&tmp.path().join("data"), &[("TXTODO_REGISTRY_DB", &db)]);
    assert_eq!(global_owner(&shared, &root), None);
    // No global registry at all: nothing to guard against.
    let empty = self::env(&tmp.path().join("nothing-here"), &[]);
    assert_eq!(global_owner(&empty, &root), None);
}
