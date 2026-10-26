//! `ListEnsure`, `ListRename` and `ListRemove` over `GlobalService` (task two-kinds-of-workspace):
//! whitebox, same-process, a real catalog over real temp folders. A list is made under the lists
//! root with the id its name gives, renamed by what it shows, and removed by moving its folder
//! aside, never by deleting it.

use std::path::Path;

use tonic::{Code, Request};
use txtodo_proto::v1::{self as pb, txtodo_server::Txtodo};

use crate::global_service::GlobalService;
use crate::list_workspace::ListName;
use crate::workspace_catalog_load_tests::{catalog_with, workspace};

fn id_of(name: &str) -> String {
    ListName::parse(name).unwrap().id().to_string()
}

/// A service over a catalog whose lists live in `<tmp>/lists`.
fn service() -> (tempfile::TempDir, GlobalService) {
    let (tmp, catalog) = catalog_with(&[], |_| {});
    catalog.set_lists_root(&tmp.path().join("lists")).unwrap();
    (tmp, GlobalService::new(catalog))
}

async fn ensure(svc: &GlobalService, name: &str) -> Result<pb::ListEnsureResponse, Code> {
    let req = pb::ListEnsureRequest { name: name.into() };
    let reply = svc.list_ensure(Request::new(req)).await;
    reply.map(tonic::Response::into_inner).map_err(|s| s.code())
}

async fn rename(
    svc: &GlobalService,
    list: &str,
    new_name: &str,
) -> Result<pb::WorkspaceInfo, Code> {
    let req = pb::ListRenameRequest {
        list: list.into(),
        new_name: new_name.into(),
    };
    let reply = svc.list_rename(Request::new(req)).await;
    reply.map(tonic::Response::into_inner).map_err(|s| s.code())
}

async fn remove(svc: &GlobalService, list: &str) -> Result<pb::ListRemoveResponse, Code> {
    let req = pb::ListRemoveRequest { list: list.into() };
    let reply = svc.list_remove(Request::new(req)).await;
    reply.map(tonic::Response::into_inner).map_err(|s| s.code())
}

async fn listed(svc: &GlobalService) -> Vec<pb::WorkspaceInfo> {
    let reply = svc
        .workspace_list(Request::new(pb::WorkspaceListRequest {}))
        .await;
    reply.unwrap().into_inner().workspaces
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_list_is_made_under_the_lists_root_with_the_id_its_name_gives() {
    let (tmp, svc) = service();
    let made = ensure(&svc, "  Work ").await.unwrap();
    assert!(made.created);
    let info = made.workspace.unwrap();
    assert_eq!(info.workspace_id, id_of("work"));
    assert_eq!(info.name, "work");
    assert_eq!(info.kind(), pb::WorkspaceKind::List);
    let dir = tmp.path().join("lists/work").canonicalize().unwrap();
    assert_eq!(Path::new(&info.root), dir);
    assert_eq!(
        std::fs::read(dir.join("todo.txt")).unwrap(),
        b"",
        "no seed task"
    );

    let again = ensure(&svc, "work").await.unwrap();
    assert!(!again.created, "a list that is here comes back unchanged");
    assert_eq!(again.workspace.unwrap().workspace_id, info.workspace_id);
    assert_eq!(listed(&svc).await.len(), 1, "one row, not two");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_bad_name_or_no_lists_folder_is_refused() {
    let (_tmp, svc) = service();
    for bad in ["", "a b", "default", "../x", "con"] {
        assert_eq!(
            ensure(&svc, bad).await.unwrap_err(),
            Code::InvalidArgument,
            "{bad:?}"
        );
    }
    let (_registry, catalog) = catalog_with(&[], |_| {});
    let bare = GlobalService::new(catalog);
    assert_eq!(
        ensure(&bare, "work").await.unwrap_err(),
        Code::FailedPrecondition
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_rename_changes_what_it_shows_and_nothing_else() {
    let (tmp, svc) = service();
    ensure(&svc, "work").await.unwrap();

    let info = rename(&svc, "work", "Projects").await.unwrap();
    assert_eq!(info.name, "projects");
    assert_eq!(info.workspace_id, id_of("work"), "the id stays");
    assert!(tmp.path().join("lists/work").is_dir(), "the folder stays");
    assert!(!tmp.path().join("lists/projects").exists());

    // Found by what it shows, and still by the name it was made under.
    assert_eq!(rename(&svc, "projects", "work").await.unwrap().name, "work");
    rename(&svc, "work", "projects").await.unwrap();
    assert_eq!(
        rename(&svc, "work", "projects").await.unwrap().name,
        "projects"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_name_that_is_taken_or_bad_is_refused_for_make_and_rename() {
    let (_tmp, svc) = service();
    ensure(&svc, "work").await.unwrap();
    ensure(&svc, "home").await.unwrap();
    rename(&svc, "work", "projects").await.unwrap();

    // `work` is shown as `projects`: making `work` again must not join it under the old name, and
    // neither `projects` (shown) nor `home` (made) may be taken by another list.
    assert_eq!(ensure(&svc, "work").await.unwrap_err(), Code::AlreadyExists);
    assert_eq!(
        ensure(&svc, "projects").await.unwrap_err(),
        Code::AlreadyExists
    );
    assert_eq!(
        rename(&svc, "home", "projects").await.unwrap_err(),
        Code::AlreadyExists
    );
    assert_eq!(
        rename(&svc, "home", "work").await.unwrap_err(),
        Code::AlreadyExists
    );
    assert_eq!(
        rename(&svc, "home", "has space").await.unwrap_err(),
        Code::InvalidArgument
    );
    assert_eq!(rename(&svc, "nope", "x").await.unwrap_err(), Code::NotFound);
    assert_eq!(
        rename(&svc, "default", "x").await.unwrap_err(),
        Code::FailedPrecondition
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_removed_list_moves_aside_and_a_new_one_starts_empty() {
    let (tmp, svc) = service();
    ensure(&svc, "work").await.unwrap();
    let dir = tmp.path().join("lists/work");
    std::fs::write(dir.join("todo.txt"), "buy milk\n").unwrap();

    let gone = remove(&svc, "work").await.unwrap();
    assert!(gone.removed);
    let backup = Path::new(&gone.moved_to);
    assert_eq!(
        std::fs::read_to_string(backup.join("todo.txt")).unwrap(),
        "buy milk\n"
    );
    assert!(
        !dir.join("todo.txt").exists(),
        "nothing of it stays in place"
    );
    assert!(listed(&svc).await.is_empty(), "no longer registered");
    assert_eq!(remove(&svc, "work").await.unwrap_err(), Code::NotFound);

    let fresh = ensure(&svc, "work").await.unwrap();
    assert!(fresh.created);
    assert_eq!(
        std::fs::read(dir.join("todo.txt")).unwrap(),
        b"",
        "starts empty"
    );
    assert_eq!(
        std::fs::read_to_string(backup.join("todo.txt")).unwrap(),
        "buy milk\n",
        "the old copy is untouched"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_default_cannot_be_removed_and_every_listing_says_list_or_repo() {
    let (tmp, svc) = service();
    let default = tmp.path().join("default");
    svc.catalog().ensure_default_workspace(&default).unwrap();
    let repo = workspace("repo-");
    svc.catalog().add_registered(repo.path()).unwrap();
    ensure(&svc, "work").await.unwrap();

    assert_eq!(
        remove(&svc, "default").await.unwrap_err(),
        Code::FailedPrecondition
    );
    let kinds: Vec<_> = listed(&svc)
        .await
        .into_iter()
        .map(|w| (w.kind(), w.name))
        .collect();
    assert!(
        kinds.contains(&(pb::WorkspaceKind::List, "default".to_owned())),
        "{kinds:?}"
    );
    assert!(
        kinds.contains(&(pb::WorkspaceKind::List, "work".to_owned())),
        "{kinds:?}"
    );
    assert_eq!(
        kinds
            .iter()
            .filter(|(k, _)| *k == pb::WorkspaceKind::Repo)
            .count(),
        1,
        "the plain folder is a repo: {kinds:?}"
    );
}
