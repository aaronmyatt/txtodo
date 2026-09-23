//! The true global mode (`daemon-global-socket`, ADR 0025): `txtodod` spawned with **no** `--dir`,
//! bound to the device-global socket path (here overridden via `$TXTODO_SOCKET` so this test never
//! touches the real machine), serving whatever the registry (`$TXTODO_REGISTRY_DB`, likewise
//! overridden) already knows about. This is the one genuinely new capability this task adds — the
//! `--dir` bridge itself is exercised by every other test file in this crate unmodified. Proves:
//! a `path` selector for a pre-registered workspace works, an absent selector also works (the
//! single-open-workspace bridge), an unknown `workspace_id` selector fails cleanly (never panics,
//! never hangs), and a second, never-registered directory auto-registers via a `path` selector,
//! after which an absent selector becomes ambiguous for workspace-scoped calls (`Health` alone
//! answers the device totals instead) while named selectors still resolve.
// Integration tests are tests: clippy.toml allows unwrap/expect in #[test] fns but not in their helpers.
#![allow(clippy::expect_used, clippy::unwrap_used)]
// A unix-domain socket is the daemon's only transport (ADR 0010); this cannot run on Windows.
#![cfg(unix)]

mod support;

use std::path::Path;
use support::global_socket::{GlobalDaemon, path_selector, seed_workspace};
use txtodo_daemon::clock::SystemClock;
use txtodo_daemon::workspace_registry::WorkspaceRegistry;
use txtodo_proto::v1::{self as pb, workspace_selector::Selector};

#[tokio::test]
async fn a_pre_registered_workspace_resolves_by_path_and_by_no_selector() {
    let registry_dir = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
    let ws_dir = seed_workspace(&registry_dir.path().join("registry.db"));
    let (_daemon, mut client) = GlobalDaemon::start(registry_dir).await;

    let by_path = client
        .health(pb::HealthRequest {
            workspace: Some(path_selector(ws_dir.path())),
        })
        .await
        .unwrap_or_else(|e| panic!("health by path: {e}"))
        .into_inner();
    assert_eq!(by_path.documents, 1);

    // No selector: the default workspace answers (task default-workspace), not the one registered
    // above. Its empty `todo.txt` is one document too, which is why this count matches.
    let by_none = client
        .health(pb::HealthRequest { workspace: None })
        .await
        .unwrap_or_else(|e| panic!("health with no selector: {e}"))
        .into_inner();
    assert_eq!(by_none.documents, 1);
}

#[tokio::test]
async fn an_unknown_workspace_id_selector_fails_cleanly_not_a_hang_or_panic() {
    let registry_dir = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
    seed_workspace(&registry_dir.path().join("registry.db"));
    let (_daemon, mut client) = GlobalDaemon::start(registry_dir).await;

    let bogus = pb::WorkspaceSelector {
        selector: Some(Selector::WorkspaceId(
            "01ARZ3NDEKTSV4RRFFQ69G5FAV".to_owned(),
        )),
    };
    let err = client
        .health(pb::HealthRequest {
            workspace: Some(bogus),
        })
        .await
        .unwrap_err();
    assert_eq!(err.code(), tonic::Code::NotFound);
}

#[tokio::test]
async fn a_second_directory_auto_registers_then_scoped_calls_need_a_selector() {
    let registry_dir = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
    let dir_a = seed_workspace(&registry_dir.path().join("registry.db"));
    let (daemon, mut client) = GlobalDaemon::start(registry_dir).await;

    // A brand-new directory, never registered before this call.
    let dir_b = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
    std::fs::write(dir_b.path().join("todo.txt"), "second\n").unwrap_or_else(|e| panic!("{e}"));
    let resp = client
        .health(pb::HealthRequest {
            workspace: Some(path_selector(dir_b.path())),
        })
        .await
        .unwrap_or_else(|e| panic!("health by path (auto-register): {e}"))
        .into_inner();
    assert_eq!(resp.documents, 1);

    // Now three workspaces are registered, the default among them, so an unselected call is not
    // ambiguous: it lands in the default (task default-workspace). It reaches the handler, which
    // refuses the empty task, rather than failing in `resolve_sole_open`.
    let err = client
        .get_notes(pb::GetNotesRequest {
            task: None,
            workspace: None,
        })
        .await
        .unwrap_err();
    assert_eq!(err.code(), tonic::Code::InvalidArgument);

    // `Health` unselected answers for the default workspace too, with the device totals beside it.
    let totals = client
        .health(pb::HealthRequest { workspace: None })
        .await
        .unwrap_or_else(|e| panic!("selector-less health with two workspaces open: {e}"))
        .into_inner();
    assert_eq!(totals.workspaces_registered, 3, "two here plus the default");
    assert_eq!(totals.workspaces_ready, 3);

    // ...but either one still resolves correctly by its own path selector.
    for dir in [&dir_a, &dir_b] {
        let resp = client
            .health(pb::HealthRequest {
                workspace: Some(path_selector(dir.path())),
            })
            .await
            .unwrap_or_else(|e| panic!("health for {}: {e}", dir.path().display()))
            .into_inner();
        assert_eq!(resp.documents, 1);
    }

    // registry.db now really does carry every root (not just an in-memory illusion).
    let registry = WorkspaceRegistry::open(&daemon.registry_db())
        .unwrap_or_else(|e| panic!("reopen registry: {e}"));
    assert_eq!(
        registry.list().unwrap_or_else(|e| panic!("{e}")).len(),
        3,
        "the pre-registered, the auto-registered and the default workspace persisted"
    );
}

/// todo `ref:test-global-daemon-acceptance`: a registered-but-broken workspace (its `oplog.db`
/// replaced with garbage, so `Store::open` refuses it) must never take the whole process down or
/// poison a healthy workspace's own resolve — `WorkspaceCatalog::open_all_registered` already
/// logs-and-skips a per-entry open failure at startup (workspace_catalog.rs's own doc), and this
/// proves the same isolation holds for a *lazy* open triggered by an in-flight RPC too, over a
/// real socket, not just in-process.
#[tokio::test]
async fn a_broken_workspace_never_affects_resolving_a_healthy_one() {
    let registry_dir = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
    let registry_db = registry_dir.path().join("registry.db");
    let healthy = seed_workspace(&registry_db);

    // A second workspace, registered but never opened by open_all_registered: its .txtodo/oplog.db
    // is garbage, so Workspace::open (via Store::open) refuses it the first time anything asks for
    // it by path — after the daemon has already started, exercising the lazy-open failure path.
    let broken = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
    std::fs::write(broken.path().join("todo.txt"), "broken\n").unwrap_or_else(|e| panic!("{e}"));
    std::fs::create_dir_all(broken.path().join(".txtodo")).unwrap_or_else(|e| panic!("{e}"));
    std::fs::write(
        broken.path().join(".txtodo").join("oplog.db"),
        b"not a sqlite file",
    )
    .unwrap_or_else(|e| panic!("{e}"));
    {
        let mut registry =
            WorkspaceRegistry::open(&registry_db).unwrap_or_else(|e| panic!("open registry: {e}"));
        registry
            .add(broken.path(), &SystemClock)
            .unwrap_or_else(|e| panic!("register broken: {e}"));
    }

    let (_daemon, mut client) = GlobalDaemon::start(registry_dir).await;

    let err = client
        .health(pb::HealthRequest {
            workspace: Some(path_selector(broken.path())),
        })
        .await
        .unwrap_err();
    assert_eq!(
        err.code(),
        tonic::Code::Internal,
        "a clean error, not a hang or crash"
    );

    // The process is still alive and the healthy workspace resolves exactly as if the broken one
    // did not exist — the failed open above touched no shared state the healthy path depends on.
    let resp = client
        .health(pb::HealthRequest {
            workspace: Some(path_selector(healthy.path())),
        })
        .await
        .unwrap_or_else(|e| panic!("healthy workspace should be unaffected: {e}"))
        .into_inner();
    assert_eq!(resp.documents, 1);
}

/// todo `ref:test-global-daemon-acceptance`: the registry survives a real process restart — not
/// just the in-process `WorkspaceRegistry::open` round trip `workspace_registry_tests.rs` already
/// covers, but a second, genuinely separate `txtodod` process pointed at the same `registry.db`
/// reporting (over `WorkspaceList`) and actually re-opening (over `Health`) every workspace the
/// first process's run had registered.
#[tokio::test]
async fn the_registry_survives_a_real_process_restart() {
    // Owned by this function, not by either GlobalDaemon below: each one's own `registry_dir`
    // field is a `tempfile::TempDir` that deletes its directory on drop, and registry_db must
    // outlive the *first* daemon's drop — an earlier version of this test passed the first
    // daemon's own tempdir path as registry_db and found 0 workspaces after "restart" because the
    // first daemon's drop had already deleted the directory registry.db lived in.
    let registry_holder = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
    let registry_db = registry_holder.path().join("registry.db");
    let dir_a = seed_workspace(&registry_db);
    let dir_b = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
    std::fs::write(dir_b.path().join("todo.txt"), "b\n").unwrap_or_else(|e| panic!("{e}"));

    {
        let socket_dir = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
        let (_daemon, mut client) =
            GlobalDaemon::start_against(socket_dir, registry_db.clone()).await;
        // Auto-registers dir_b for real, into the on-disk registry this process' Drop will outlive.
        client
            .health(pb::HealthRequest {
                workspace: Some(path_selector(dir_b.path())),
            })
            .await
            .unwrap_or_else(|e| panic!("register b: {e}"));
    } // daemon killed here (Drop) — a real process exit, not a graceful shutdown

    let restarted_dir = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
    let (_restarted, mut client) = GlobalDaemon::start_against(restarted_dir, registry_db).await;

    let listed = client
        .workspace_list(pb::WorkspaceListRequest {})
        .await
        .unwrap_or_else(|e| panic!("workspace_list on restart: {e}"))
        .into_inner()
        .workspaces;
    assert_eq!(
        listed.len(),
        3,
        "both workspaces and the default survived the restart: {listed:?}"
    );

    for dir in [&dir_a, &dir_b] {
        let resp = client
            .health(pb::HealthRequest {
                workspace: Some(path_selector(dir.path())),
            })
            .await
            .unwrap_or_else(|e| {
                panic!(
                    "restarted daemon should re-open {}: {e}",
                    dir.path().display()
                )
            })
            .into_inner();
        assert_eq!(resp.documents, 1);
    }
}

/// Task layout-client-gaps: `WorkspaceList` carries each workspace's layout, so a client that
/// trusts it (instead of a second `WorkspaceLayout` call per entry) opens the right root list.
#[tokio::test]
async fn workspace_list_carries_each_workspaces_layout() {
    let registry_dir = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
    let registry_db = registry_dir.path().join("registry.db");
    let custom = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
    std::fs::write(
        custom.path().join("txtodo.toml"),
        "todo_file = \"work.txt\"\n",
    )
    .unwrap_or_else(|e| panic!("{e}"));
    std::fs::write(custom.path().join("work.txt"), "plan\n").unwrap_or_else(|e| panic!("{e}"));
    {
        let mut registry =
            WorkspaceRegistry::open(&registry_db).unwrap_or_else(|e| panic!("open registry: {e}"));
        registry
            .add(custom.path(), &SystemClock)
            .unwrap_or_else(|e| panic!("register: {e}"));
    }
    let plain = seed_workspace(&registry_db);
    let (_daemon, mut client) = GlobalDaemon::start(registry_dir).await;

    let listed = client
        .workspace_list(pb::WorkspaceListRequest {})
        .await
        .unwrap_or_else(|e| panic!("workspace_list: {e}"))
        .into_inner()
        .workspaces;
    let canon = |p: &Path| p.canonicalize().unwrap_or_else(|e| panic!("{e}"));
    let find = |root: &Path| {
        let root = canon(root).display().to_string();
        listed
            .iter()
            .find(|w| w.root == root)
            .unwrap_or_else(|| panic!("{root} not listed in {listed:?}"))
    };
    let custom_info = find(custom.path());
    assert_eq!(custom_info.todo_file, "work.txt");
    assert_eq!(custom_info.refs_dir, "tasks");
    let plain_info = find(plain.path());
    assert_eq!(plain_info.todo_file, "todo.txt");
    assert!(
        listed
            .iter()
            .all(|w| !w.todo_file.is_empty() && !w.refs_dir.is_empty()),
        "no entry reads as an older daemon: {listed:?}"
    );
}
