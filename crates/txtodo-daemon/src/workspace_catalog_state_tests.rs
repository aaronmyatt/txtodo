//! `WorkspaceList`/`Health` load reporting and the recency load order (task `daemon-early-bind`),
//! split from `workspace_catalog_load_tests.rs` for its file budget; shares its helpers.

use crate::clock::FakeClock;
use crate::workspace_catalog::WorkspaceCatalog;
use crate::workspace_catalog_load_tests::{
    Gate, catalog_with, open_args, select, state_of, wait_for, workspace,
};
use crate::workspace_load::LoadState;
use crate::workspace_registry::WorkspaceRegistry;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use txtodo_proto::v1 as pb;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_service_answers_list_and_health_while_a_workspace_is_still_opening() {
    use crate::global_service::GlobalService;
    use txtodo_proto::v1::txtodo_server::Txtodo;

    let slow = workspace("still-opening-");
    let gate = Arc::new(Gate::default());
    let hook_gate = Arc::clone(&gate);
    let (_registry_dir, catalog) = catalog_with(&[slow.path()], move |_| hook_gate.wait());
    let order = catalog.queue_registered();
    let _loader = catalog
        .spawn_loader(order)
        .unwrap_or_else(|e| panic!("loader: {e}"));
    wait_for("the open to start", || {
        state_of(&catalog, slow.path()) == Some(LoadState::Loading)
    });
    let service = GlobalService::new(Arc::clone(&catalog));

    // The cold-boot promise: the daemon answers, and says what it is still doing, before the last
    // open finishes.
    let listed = service
        .workspace_list(tonic::Request::new(pb::WorkspaceListRequest {}))
        .await
        .unwrap_or_else(|e| panic!("workspace_list: {e}"))
        .into_inner();
    assert_eq!(listed.workspaces.len(), 1);
    assert_eq!(
        listed.workspaces[0].load_state,
        pb::WorkspaceLoadState::Loading as i32
    );
    let health = service
        .health(tonic::Request::new(pb::HealthRequest { workspace: None }))
        .await
        .unwrap_or_else(|e| panic!("health with nothing ready must still answer: {e}"))
        .into_inner();
    assert_eq!(
        (
            health.workspaces_registered,
            health.workspaces_ready,
            health.workspaces_loading
        ),
        (1, 0, 1)
    );

    gate.release();
    wait_for("the open to finish", || catalog.load_pending() == 0);
    let listed = service
        .workspace_list(tonic::Request::new(pb::WorkspaceListRequest {}))
        .await
        .unwrap_or_else(|e| panic!("workspace_list: {e}"))
        .into_inner();
    assert_eq!(
        listed.workspaces[0].load_state,
        pb::WorkspaceLoadState::Ready as i32
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_workspace_a_request_used_loads_before_one_only_its_file_mtime_favours() {
    let used = workspace("used-");
    let newer_file = workspace("newer-file-");
    // `newer_file` was edited a moment ago; `used` a long time ago, but a request resolved it.
    std::fs::File::options()
        .write(true)
        .open(used.path().join("todo.txt"))
        .and_then(|f| f.set_modified(SystemTime::now() - Duration::from_secs(90_000)))
        .unwrap_or_else(|e| panic!("{e}"));
    let registry_dir = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
    let mut registry = WorkspaceRegistry::open(&registry_dir.path().join("registry.db"))
        .unwrap_or_else(|e| panic!("open registry: {e}"));
    let clock = FakeClock::new(4_000_000_000_000); // far later than any real mtime
    for root in [used.path(), newer_file.path()] {
        registry
            .add(root, &clock)
            .unwrap_or_else(|e| panic!("register: {e}"));
    }
    let catalog = Arc::new(WorkspaceCatalog::new(
        registry,
        open_args(),
        Arc::new(clock),
    ));
    assert_eq!(
        catalog.queue_registered()[0].1,
        newer_file.path().canonicalize().unwrap_or_default(),
        "before any request, the fresher file wins"
    );

    catalog
        .resolve(Some(&select(used.path())))
        .unwrap_or_else(|e| panic!("resolve: {e}"));

    assert_eq!(
        catalog.queue_registered()[0].1,
        used.path().canonicalize().unwrap_or_default(),
        "a workspace a request used is opened first on the next cold boot"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn lint_runs_the_cli_check_over_the_documents_bytes() {
    use crate::global_service::GlobalService;
    use txtodo_proto::v1::txtodo_server::Txtodo;

    let ws = workspace("lint-");
    std::fs::write(
        ws.path().join("todo.txt"),
        format!("short\n{}\n", "a".repeat(101)),
    )
    .unwrap_or_else(|e| panic!("{e}"));
    let (_registry_dir, catalog) = catalog_with(&[], |_| {});
    let service = GlobalService::new(catalog);

    let response = service
        .lint(tonic::Request::new(pb::LintRequest {
            path: "todo.txt".to_owned(),
            workspace: Some(select(ws.path())),
        }))
        .await
        .unwrap_or_else(|e| panic!("lint: {e}"))
        .into_inner();

    assert_eq!(
        response.findings,
        vec![pb::LintFinding {
            line: 2,
            message: "101 chars, over the 100-char hint".to_owned()
        }]
    );
    let missing = service
        .lint(tonic::Request::new(pb::LintRequest {
            path: "nope.txt".to_owned(),
            workspace: Some(select(ws.path())),
        }))
        .await
        .expect_err("no such document");
    assert_eq!(missing.code(), tonic::Code::NotFound);
}
