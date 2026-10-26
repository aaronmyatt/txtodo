//! The repo scan through a real catalog and the `Repos` RPC (task two-kinds-of-workspace): clones
//! found under a scan root are registered, `.txtodo/` goes into their own exclude, a scan twice
//! changes nothing, a removal sticks, and a bad root is refused.

use std::path::{Path, PathBuf};

use tonic::{Code, Request};
use txtodo_proto::v1::{self as pb, txtodo_server::Txtodo};

use crate::global_service::GlobalService;
use crate::workspace_catalog_load_tests::catalog_with;

/// A clone at `rel` under `root` holding a `todo.txt`.
fn clone_at(root: &Path, rel: &str) -> PathBuf {
    let dir = root.join(rel);
    std::fs::create_dir_all(dir.join(".git/info")).unwrap();
    std::fs::write(dir.join("todo.txt"), "a task\n").unwrap();
    dir.canonicalize().unwrap()
}

fn registered(catalog: &crate::workspace_catalog::WorkspaceCatalog) -> Vec<PathBuf> {
    let entries = catalog.list_registered_entries().unwrap();
    let mut roots: Vec<PathBuf> = entries.into_iter().map(|e| e.root).collect();
    roots.sort();
    roots
}

async fn repos(svc: &GlobalService, req: pb::ReposRequest) -> Result<pb::ReposResponse, Code> {
    let reply = svc.repos(Request::new(req)).await;
    reply.map(tonic::Response::into_inner).map_err(|s| s.code())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_scan_registers_the_clones_it_finds_and_keeps_txtodo_out_of_their_git() {
    let (_state, catalog) = catalog_with(&[], |_| {});
    let home = tempfile::tempdir().unwrap();
    let (a, b) = (
        clone_at(home.path(), "code/a"),
        clone_at(home.path(), "code/b"),
    );
    std::fs::create_dir_all(home.path().join("code/not-a-clone")).unwrap();
    std::fs::write(home.path().join("code/not-a-clone/todo.txt"), "x\n").unwrap();
    catalog
        .edit_scan_roots(&[home.path().display().to_string()], &[])
        .unwrap();

    let guard = catalog.try_begin_scan().expect("nothing is scanning");
    assert!(catalog.try_begin_scan().is_none(), "one scan at a time");
    let report = catalog.run_scan(guard).unwrap();

    assert_eq!((report.found, report.registered), (2, 2));
    assert_eq!(registered(&catalog), [a.clone(), b.clone()]);
    for repo in [&a, &b] {
        let exclude = std::fs::read_to_string(repo.join(".git/info/exclude")).unwrap();
        assert!(exclude.lines().any(|l| l == ".txtodo/"), "{exclude}");
        assert!(!repo.join(".txtodo").exists(), "registering opens nothing");
        assert_readied_for_git(repo);
    }
    assert!(
        catalog.try_begin_scan().is_some(),
        "the guard was dropped: free again"
    );
}

/// The lists merge line by line and the merge driver is set, in the clone's own `.git`.
fn assert_readied_for_git(repo: &Path) {
    let attributes = std::fs::read_to_string(repo.join(".git/info/attributes")).unwrap();
    assert!(attributes.contains("todo.txt merge=txtodo"), "{attributes}");
    let config = std::fs::read_to_string(repo.join(".git/config")).unwrap();
    assert!(
        config.contains("driver = txtodo merge-driver %O %A %B"),
        "{config}"
    );
    assert!(
        !repo.join(".gitattributes").exists(),
        "nothing tracked is written"
    );
}

/// A repo registered before this was automatic (or one whose setup was undone) is readied again by
/// the next scan, with nothing else about it changing.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_scan_readies_a_repo_that_was_registered_before_and_lacks_the_setup() {
    let (_state, catalog) = catalog_with(&[], |_| {});
    let home = tempfile::tempdir().unwrap();
    let repo = clone_at(home.path(), "a");
    catalog
        .edit_scan_roots(&[home.path().display().to_string()], &[])
        .unwrap();
    catalog.run_scan(catalog.try_begin_scan().unwrap()).unwrap();
    std::fs::remove_file(repo.join(".git/config")).unwrap();
    std::fs::remove_file(repo.join(".git/info/attributes")).unwrap();

    let again = catalog.run_scan(catalog.try_begin_scan().unwrap()).unwrap();
    assert_eq!(again.registered, 0, "already registered");
    assert_readied_for_git(&repo);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_second_scan_finds_them_again_and_registers_nothing() {
    let (_state, catalog) = catalog_with(&[], |_| {});
    let home = tempfile::tempdir().unwrap();
    clone_at(home.path(), "a");
    catalog
        .edit_scan_roots(&[home.path().display().to_string()], &[])
        .unwrap();
    let first = catalog.run_scan(catalog.try_begin_scan().unwrap()).unwrap();
    let again = catalog.run_scan(catalog.try_begin_scan().unwrap()).unwrap();
    assert_eq!((first.registered, again.found, again.registered), (1, 1, 0));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_repo_the_person_removed_does_not_come_back() {
    let (_state, catalog) = catalog_with(&[], |_| {});
    let home = tempfile::tempdir().unwrap();
    let repo = clone_at(home.path(), "a");
    catalog
        .edit_scan_roots(&[home.path().display().to_string()], &[])
        .unwrap();
    catalog.run_scan(catalog.try_begin_scan().unwrap()).unwrap();
    let id = catalog.list_registered_entries().unwrap()[0].id;
    assert!(catalog.remove_registered(id).unwrap());

    let report = catalog.run_scan(catalog.try_begin_scan().unwrap()).unwrap();
    assert_eq!((report.found, report.registered), (1, 0));
    assert!(!registered(&catalog).contains(&repo));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_clone_nested_in_another_is_its_own_workspace() {
    // The walker leaves a nested checkout to its own workspace, so the registry sees no overlap.
    let (_state, catalog) = catalog_with(&[], |_| {});
    let home = tempfile::tempdir().unwrap();
    let outer = clone_at(home.path(), "outer");
    let inner = clone_at(home.path(), "outer/inner");
    catalog
        .edit_scan_roots(&[home.path().display().to_string()], &[])
        .unwrap();
    let report = catalog.run_scan(catalog.try_begin_scan().unwrap()).unwrap();
    assert_eq!((report.found, report.registered), (2, 2));
    assert_eq!(registered(&catalog), [outer, inner]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_daemons_own_data_folder_is_never_scanned() {
    let (state, catalog) = catalog_with(&[], |_| {});
    let home = tempfile::tempdir().unwrap();
    let data = home.path().join("data");
    catalog.set_lists_root(&data.join("lists")).unwrap();
    clone_at(&data, "lists/work");
    clone_at(home.path(), "code/a");
    catalog
        .edit_scan_roots(&[home.path().display().to_string()], &[])
        .unwrap();
    let report = catalog.run_scan(catalog.try_begin_scan().unwrap()).unwrap();
    assert_eq!((report.found, report.registered), (1, 1));
    drop(state);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_repos_rpc_edits_the_roots_scans_and_answers_the_state() {
    let (_state, catalog) = catalog_with(&[], |_| {});
    let svc = GlobalService::new(catalog);
    let home = tempfile::tempdir().unwrap();
    clone_at(home.path(), "a");
    clone_at(home.path(), "b");
    let root = home.path().display().to_string();

    let none = repos(&svc, pb::ReposRequest::default()).await.unwrap();
    assert!(none.roots.is_empty() && none.last_scan_ms == 0 && !none.running);

    let done = pb::ReposRequest {
        add_roots: vec![root.clone()],
        scan: true,
        wait: true,
        ..pb::ReposRequest::default()
    };
    let got = repos(&svc, done).await.unwrap();
    assert_eq!(got.roots.len(), 1);
    assert_eq!(got.roots[0], root);
    assert_eq!((got.found, got.registered), (2, 2));
    assert!(got.last_scan_ms > 0 && !got.running, "{got:?}");

    let gone = pb::ReposRequest {
        remove_roots: vec![root],
        ..pb::ReposRequest::default()
    };
    assert!(repos(&svc, gone).await.unwrap().roots.is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_scan_root_must_be_an_absolute_path() {
    let (_state, catalog) = catalog_with(&[], |_| {});
    let svc = GlobalService::new(catalog);
    let req = pb::ReposRequest {
        add_roots: vec!["relative/dir".into()],
        ..pb::ReposRequest::default()
    };
    assert_eq!(repos(&svc, req).await.unwrap_err(), Code::InvalidArgument);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_registry_given_its_default_roots_once_keeps_them_removed() {
    let (_state, catalog) = catalog_with(&[], |_| {});
    catalog.seed_scan_roots(&[PathBuf::from("/home/a")]);
    catalog.seed_scan_roots(&[PathBuf::from("/home/b")]);
    assert_eq!(catalog.scan_roots().unwrap(), ["/home/a"]);
    catalog
        .edit_scan_roots(&[], &["/home/a".to_owned()])
        .unwrap();
    catalog.seed_scan_roots(&[PathBuf::from("/home/a")]);
    assert!(catalog.scan_roots().unwrap().is_empty());
}
