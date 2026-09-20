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

/// Code review 2026-09-20, finding 13: `last_active_ms` used to win outright, so a workspace whose
/// `todo.txt` was edited today in an editor loaded after one a request touched a week ago.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_file_edited_today_loads_before_a_workspace_a_request_used_a_week_ago() {
    const DAY_MS: u64 = 86_400_000;
    let now_ms = u64::try_from(
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis(),
    )
    .unwrap_or_default();
    let edited_today = workspace("edited-today-");
    let used_last_week = workspace("used-last-week-");
    let registry_dir = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
    let mut registry = WorkspaceRegistry::open(&registry_dir.path().join("registry.db"))
        .unwrap_or_else(|e| panic!("open registry: {e}"));
    let clock = Arc::new(FakeClock::new(now_ms - 30 * DAY_MS));
    for root in [edited_today.path(), used_last_week.path()] {
        registry
            .add(root, clock.as_ref())
            .unwrap_or_else(|e| panic!("register: {e}"));
    }
    let catalog = WorkspaceCatalog::new(registry, open_args(), Arc::clone(&clock) as _);

    // A request used the first a month ago and the second a week ago...
    catalog
        .resolve(Some(&select(edited_today.path())))
        .unwrap_or_else(|e| panic!("resolve: {e}"));
    clock.advance_ms(23 * DAY_MS);
    catalog
        .resolve(Some(&select(used_last_week.path())))
        .unwrap_or_else(|e| panic!("resolve: {e}"));
    // ...and the second's file is old, while the first's was written just now (by `workspace`).
    std::fs::File::options()
        .write(true)
        .open(used_last_week.path().join("todo.txt"))
        .and_then(|f| f.set_modified(SystemTime::now() - Duration::from_secs(40 * 86_400)))
        .unwrap_or_else(|e| panic!("{e}"));

    assert_eq!(
        catalog.queue_registered()[0].1,
        edited_today.path().canonicalize().unwrap_or_default(),
        "the file edited today is the one in use"
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

/// Code review 2026-09-20, finding 9: with no selector, `Health` went on to `resolve_sole_open`,
/// which refuses 0 or 2+ open workspaces ("ambiguous"). The totals are about the device, not about
/// one workspace, so a selector-less `Health` must always answer: the one open workspace's details
/// when there is exactly one (what the `--dir` bridge and the tests read), else the totals alone.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_selector_less_health_answers_with_no_or_several_open_workspaces() {
    use crate::global_service::GlobalService;
    use txtodo_proto::v1::txtodo_server::Txtodo;

    let (one, two) = (workspace("health-one-"), workspace("health-two-"));
    let (_registry_dir, catalog) = catalog_with(&[], |_| {});
    let service = GlobalService::new(Arc::clone(&catalog));
    let ask = || tonic::Request::new(pb::HealthRequest { workspace: None });

    let none_open = service
        .health(ask())
        .await
        .unwrap_or_else(|e| panic!("health with nothing open must still answer: {e}"))
        .into_inner();
    assert_eq!(none_open.workspaces_ready, 0);
    assert!(!none_open.version.is_empty());

    for dir in [&one, &two] {
        let catalog = Arc::clone(&catalog);
        let selector = select(dir.path());
        tokio::task::spawn_blocking(move || catalog.resolve(Some(&selector)).map(|_| ()))
            .await
            .unwrap_or_else(|e| panic!("open task: {e}"))
            .unwrap_or_else(|e| panic!("open: {e}"));
    }
    let two_open = service
        .health(ask())
        .await
        .unwrap_or_else(|e| panic!("health with two open must still answer: {e}"))
        .into_inner();
    assert_eq!(
        (two_open.workspaces_registered, two_open.workspaces_ready),
        (2, 2)
    );
}

/// Code review 2026-09-20, finding 11: the path fast path compared roots by taking a read lock on
/// every open workspace, so one workspace held under a write lock (a slow registration, a
/// migration) stalled a request that named a different one.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_write_locked_workspace_does_not_stall_a_path_lookup_of_another() {
    let (busy, other) = (workspace("busy-"), workspace("other-"));
    let (_registry_dir, catalog) = catalog_with(&[], |_| {});
    let mut opened = Vec::new();
    for dir in [&busy, &other] {
        let catalog = Arc::clone(&catalog);
        let selector = select(dir.path());
        let ws = tokio::task::spawn_blocking(move || catalog.resolve(Some(&selector)))
            .await
            .unwrap_or_else(|e| panic!("open task: {e}"))
            .unwrap_or_else(|e| panic!("open: {e}"));
        opened.push(ws);
    }
    let _held = opened[0].write().unwrap_or_else(|e| e.into_inner());

    // On its own thread: before the fix this blocked on `busy`'s lock and never reported back.
    // A path that is not open has to be compared with every open root, `busy`'s included (the
    // map's order is random, so only this lookup is sure to reach `busy`); then `other` itself.
    let not_open = workspace("not-open-");
    let (done_tx, done_rx) = std::sync::mpsc::channel();
    let lookup_catalog = Arc::clone(&catalog);
    let selectors = (select(not_open.path()), select(other.path()));
    std::thread::spawn(move || {
        let miss = lookup_catalog.resolve_without_waiting(Some(&selectors.0));
        let hit = lookup_catalog.resolve_without_waiting(Some(&selectors.1));
        let _ = done_tx.send((miss.is_none(), matches!(hit, Some(Ok(_)))));
    });
    let (missed, found) = done_rx
        .recv_timeout(Duration::from_secs(30))
        .unwrap_or_else(|_| panic!("a path lookup waited on `busy`'s write lock"));
    assert!(missed, "a path that is not open takes the slow path");
    assert!(found, "`other` is open, so the fast path answers it");
}
