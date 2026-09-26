//! Task sync-drift line 8, two real daemons over LAN: B's copy of A's workspace goes wrong the
//! way the Evidence did (A refuses one of B's runs, so B's later ops, duplicates of A's lines
//! among them, wait behind it and stay on B alone). `WorkspaceRejoin` on B moves B's copy into a
//! backup folder beside it and takes A's: B ends with A's lines only, the backup holds B's old
//! copy, and B's next edit reaches A.
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::print_stderr)]
#![cfg(unix)]

mod support;

use std::path::Path;
use std::time::{Duration, Instant};
use support::multi::{MultiClient, MultiWorkspaceDaemon, file_at};
use txtodo_proto::v1::{self as pb, mutation, workspace_selector::Selector};

const DEADLINE: Duration = Duration::from_secs(90);

fn code(r: &pb::PairOfferResponse) -> String {
    serde_json::json!({
        "device": r.device,
        "group_id": r.group_id,
        "x25519_pub": r.x25519_pub,
        "endpoint": r.endpoint,
        "nonce": r.nonce,
        "identity_mode": r.identity_mode,
    })
    .to_string()
}

/// `a` offers, `b` joins, both say "my own device".
async fn pair(a: &mut MultiClient, b: &mut MultiClient) {
    let offer = a
        .pair_offer(pb::PairOfferRequest { workspace: None })
        .await
        .unwrap()
        .into_inner();
    let accept = pb::PairAcceptRequest {
        code: code(&offer),
        workspace: None,
    };
    let sas_b = b.pair_accept(accept).await.unwrap().into_inner().sas;
    let start = Instant::now();
    loop {
        let req = pb::PairAwaitPeerRequest { workspace: None };
        let sas_a = a.pair_await_peer(req).await.unwrap().into_inner().sas;
        if !sas_a.is_empty() {
            assert_eq!(sas_a, sas_b);
            break;
        }
        assert!(start.elapsed() < DEADLINE, "no joiner reached A");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    for client in [a, b] {
        let req = pb::PairConfirmRequest {
            workspace: None,
            own_device: true,
        };
        client.pair_confirm_sas(req).await.unwrap();
    }
}

fn at(root: &str) -> Option<pb::WorkspaceSelector> {
    Some(pb::WorkspaceSelector {
        selector: Some(Selector::Path(root.to_owned())),
    })
}

async fn add(client: &mut MultiClient, root: &str, path: &str, line: &str) {
    let req = pb::ApplyRequest {
        path: path.into(),
        mutations: vec![pb::Mutation {
            kind: Some(mutation::Kind::Add(pb::Add { line: line.into() })),
        }],
        workspace: at(root),
        ..pb::ApplyRequest::default()
    };
    client.apply(req).await.unwrap();
}

async fn text(client: &mut MultiClient, root: &str) -> String {
    String::from_utf8(file_at(client, Path::new(root)).await).unwrap()
}

/// Waits until `root`'s `todo.txt` on `client` passes `ok`; returns that text.
async fn wait_text(
    client: &mut MultiClient,
    root: &str,
    what: &str,
    ok: impl Fn(&str) -> bool,
    log: &dyn Fn() -> String,
) -> String {
    let start = Instant::now();
    loop {
        let now = text(client, root).await;
        if ok(&now) {
            return now;
        }
        assert!(
            start.elapsed() < DEADLINE,
            "{what}; last:\n{now}\n{}",
            log()
        );
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

/// The Remote mirror on `client` whose list holds `needle`.
async fn remote_with(client: &mut MultiClient, needle: &str, log: &dyn Fn() -> String) -> String {
    let start = Instant::now();
    loop {
        let listed = client.workspace_list(pb::WorkspaceListRequest {}).await;
        for w in listed.unwrap().into_inner().workspaces {
            if w.is_remote && text(client, &w.root).await.contains(needle) {
                return w.root;
            }
        }
        assert!(
            start.elapsed() < DEADLINE,
            "no mirror with {needle}\n{}",
            log()
        );
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

/// Waits until A says its sync from B is stuck on `file`.
async fn wait_stuck(a: &mut MultiClient, file: &str, log: &dyn Fn() -> String) {
    let start = Instant::now();
    loop {
        let req = pb::SyncStatusRequest { workspace: None };
        let peers = a.sync_status(req).await.unwrap().into_inner().peers;
        if peers.iter().flat_map(|p| &p.stuck).any(|s| s.file == file) {
            return;
        }
        assert!(
            start.elapsed() < DEADLINE,
            "A never refused {file}\n{}",
            log()
        );
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

fn count(text: &str, needle: &str) -> usize {
    text.matches(needle).count()
}

async fn rejoin(b: &mut MultiClient, id: &str, dry_run: bool) -> pb::WorkspaceRejoinResponse {
    let req = pb::WorkspaceRejoinRequest {
        workspace_id: id.to_owned(),
        dry_run,
    };
    b.workspace_rejoin(req).await.unwrap().into_inner()
}

/// A's workspace: two lines, and a plain file where B's `blocked/` folder would go.
async fn a_workspace(a: &mut MultiClient, dir: &Path) -> pb::WorkspaceInfo {
    std::fs::write(
        dir.join("todo.txt"),
        "(A) first from a\n(B) second from a\n",
    )
    .unwrap();
    std::fs::write(dir.join("blocked"), "a file, not a folder\n").unwrap();
    let req = pb::WorkspaceAddRequest {
        root: dir.display().to_string(),
    };
    let info = a.workspace_add(req).await.unwrap().into_inner();
    text(a, &info.root).await;
    info
}

/// Both clients and the workspace's folder on each side.
struct Two {
    a: MultiClient,
    b: MultiClient,
    root_a: String,
    root_b: String,
}

/// A takes B's first line; then a run A cannot take (no folder for it), and B's duplicates of
/// A's lines wait behind it on B. Returns B's list as it then reads.
async fn break_bs_copy(t: &mut Two, logs: &dyn Fn() -> String) -> String {
    add(&mut t.b, &t.root_b, "todo.txt", "b before").await;
    let has_b = |text: &str| text.contains("b before");
    wait_text(&mut t.a, &t.root_a, "A never got B's line", has_b, logs).await;
    std::fs::create_dir_all(Path::new(&t.root_b).join("blocked")).unwrap();
    add(&mut t.b, &t.root_b, "blocked/todo.txt", "stuck on b").await;
    wait_stuck(&mut t.a, "blocked/todo.txt", logs).await;
    add(&mut t.b, &t.root_b, "todo.txt", "(A) first from a").await;
    add(&mut t.b, &t.root_b, "todo.txt", "(B) second from a").await;
    let before = text(&mut t.b, &t.root_b).await;
    assert_eq!(count(&before, "first from a"), 2, "duplicates:\n{before}");
    assert!(before.contains("b before"), "{before}");
    before
}

/// The backup holds B's old copy: the duplicates, the refused sub-list, and the old store.
fn assert_backup_holds_bs_copy(done: &pb::WorkspaceRejoinResponse) {
    let backup = Path::new(&done.backup_dir);
    let old = std::fs::read_to_string(backup.join("todo.txt")).unwrap();
    assert_eq!(count(&old, "first from a"), 2, "{old}");
    let stuck = std::fs::read_to_string(backup.join("blocked/todo.txt")).unwrap();
    assert!(stuck.contains("stuck on b"), "{stuck}");
    assert!(backup.join(".txtodo/oplog.db").is_file(), "the old store");
}

fn only_as_lines(text: &str) -> bool {
    let once = |line: &str| count(text, line) == 1;
    once("first from a") && once("second from a") && once("b before") && !text.contains("stuck")
}

#[tokio::test]
async fn a_rejoin_drops_bs_copy_into_a_backup_and_takes_as() {
    let (dir_a, dir_b, work) = (
        tempfile::tempdir().unwrap(),
        tempfile::tempdir().unwrap(),
        tempfile::tempdir().unwrap(),
    );
    let (a, mut client_a) = MultiWorkspaceDaemon::start_with_args(dir_a, &[]).await;
    let (b, mut client_b) = MultiWorkspaceDaemon::start_with_args(dir_b, &[]).await;
    let logs = || format!("--- A ---\n{}\n--- B ---\n{}", a.log_tail(), b.log_tail());
    let w = a_workspace(&mut client_a, work.path()).await;
    pair(&mut client_a, &mut client_b).await;
    let root_b = remote_with(&mut client_b, "second from a", &logs).await;
    let mut t = Two {
        a: client_a,
        b: client_b,
        root_a: w.root.clone(),
        root_b,
    };
    let before = break_bs_copy(&mut t, &logs).await;

    let plan = rejoin(&mut t.b, &w.workspace_id, true).await;
    assert_eq!(plan.moved, [".txtodo", "blocked/todo.txt", "todo.txt"]);
    assert_eq!(plan.offering_devices.len(), 1, "{plan:?}");
    assert_eq!(
        text(&mut t.b, &t.root_b).await,
        before,
        "a dry run moves nothing"
    );

    let done = rejoin(&mut t.b, &w.workspace_id, false).await;
    assert_eq!(
        done.workspace.as_ref().unwrap().root,
        t.root_b,
        "same folder"
    );
    assert_backup_holds_bs_copy(&done);
    wait_text(
        &mut t.b,
        &t.root_b,
        "B never took A's copy",
        only_as_lines,
        &logs,
    )
    .await;
    assert!(!Path::new(&t.root_b).join("blocked/todo.txt").exists());

    add(&mut t.b, &t.root_b, "todo.txt", "b after rejoin").await;
    let reached = |text: &str| text.contains("b after rejoin");
    let at_a = wait_text(
        &mut t.a,
        &t.root_a,
        "B's line never reached A",
        reached,
        &logs,
    )
    .await;
    assert_eq!(
        count(&at_a, "first from a"),
        1,
        "A never got B's duplicates:\n{at_a}"
    );
    assert!(!at_a.contains("stuck on b"), "{at_a}");
}
