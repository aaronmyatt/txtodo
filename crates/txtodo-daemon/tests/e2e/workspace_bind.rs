//! Task spike-txtodo-should-merge-remote-workspa, two real daemons over LAN: A and B each hold a
//! clone of one project, so each registered its own workspace for it. B mirrors A's as a Remote
//! workspace, and its own clone never sees A's list. `WorkspaceBind` on B moves the mirror into
//! the clone: the clone's own list goes to a backup beside it, the clone now reads A's list, and
//! edits flow both ways under A's workspace id.
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::print_stderr)]
#![cfg(unix)]

use crate::support;

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
    let offer = a.pair_offer(pb::PairOfferRequest { workspace: None }).await;
    let offer = offer.unwrap().into_inner();
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

async fn add(client: &mut MultiClient, root: &str, line: &str) {
    let req = pb::ApplyRequest {
        path: "todo.txt".into(),
        mutations: vec![pb::Mutation {
            kind: Some(mutation::Kind::Add(pb::Add { line: line.into() })),
        }],
        workspace: Some(pb::WorkspaceSelector {
            selector: Some(Selector::Path(root.to_owned())),
        }),
        ..pb::ApplyRequest::default()
    };
    client.apply(req).await.unwrap();
}

async fn text(client: &mut MultiClient, root: &str) -> String {
    String::from_utf8(file_at(client, Path::new(root)).await).unwrap()
}

/// Waits until `root`'s `todo.txt` on `client` holds `needle`; returns that text.
async fn wait_for(
    client: &mut MultiClient,
    root: &str,
    needle: &str,
    log: &dyn Fn() -> String,
) -> String {
    let start = Instant::now();
    loop {
        let now = text(client, root).await;
        if now.contains(needle) {
            return now;
        }
        let late = start.elapsed() >= DEADLINE;
        assert!(
            !late,
            "{needle} never reached {root}; last:\n{now}\n{}",
            log()
        );
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

/// The Remote mirror on `client` whose list holds `needle`.
async fn remote_with(
    client: &mut MultiClient,
    needle: &str,
    log: &dyn Fn() -> String,
) -> pb::WorkspaceInfo {
    let start = Instant::now();
    loop {
        let listed = client.workspace_list(pb::WorkspaceListRequest {}).await;
        for w in listed.unwrap().into_inner().workspaces {
            if w.is_remote && text(client, &w.root).await.contains(needle) {
                return w;
            }
        }
        let late = start.elapsed() >= DEADLINE;
        assert!(!late, "no mirror with {needle}\n{}", log());
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

/// A clone at `<dir>/project` holding `list`, registered.
async fn clone(client: &mut MultiClient, dir: &Path, list: &str) -> pb::WorkspaceInfo {
    let root = dir.join("project");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("todo.txt"), list).unwrap();
    let req = pb::WorkspaceAddRequest {
        root: root.display().to_string(),
    };
    let info = client.workspace_add(req).await.unwrap().into_inner();
    text(client, &info.root).await;
    info
}

async fn bind(
    b: &mut MultiClient,
    id: &str,
    root: &str,
    dry_run: bool,
) -> pb::WorkspaceBindResponse {
    let req = pb::WorkspaceBindRequest {
        workspace_id: id.to_owned(),
        root: root.to_owned(),
        dry_run,
    };
    b.workspace_bind(req).await.unwrap().into_inner()
}

#[tokio::test]
async fn a_bind_moves_as_mirror_into_bs_clone_and_edits_flow_both_ways() {
    let (dir_a, dir_b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let (work_a, work_b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let (a, mut client_a) = MultiWorkspaceDaemon::start_with_args(dir_a, &[]).await;
    let (b, mut client_b) = MultiWorkspaceDaemon::start_with_args(dir_b, &[]).await;
    let logs = || format!("--- A ---\n{}\n--- B ---\n{}", a.log_tail(), b.log_tail());
    let on_a = clone(&mut client_a, work_a.path(), "(A) shared from a\n").await;
    let on_b = clone(
        &mut client_b,
        work_b.path(),
        "(A) shared from a\n(Z) b's own\n",
    )
    .await;
    pair(&mut client_a, &mut client_b).await;
    let mirror = remote_with(&mut client_b, "shared from a", &logs).await;
    assert_eq!(
        mirror.workspace_id, on_a.workspace_id,
        "B mirrors A's own id"
    );

    let plan = bind(&mut client_b, &mirror.workspace_id, &on_b.root, true).await;
    assert_eq!(plan.replaced_workspace_id, on_b.workspace_id);
    assert!(
        text(&mut client_b, &on_b.root).await.contains("b's own"),
        "dry run"
    );

    let done = bind(&mut client_b, &mirror.workspace_id, &on_b.root, false).await;
    let info = done.workspace.unwrap();
    assert_eq!(
        (info.root.as_str(), info.is_remote),
        (on_b.root.as_str(), false)
    );
    let old = std::fs::read_to_string(Path::new(&done.backup_dir).join("todo.txt")).unwrap();
    assert!(
        old.contains("b's own"),
        "the clone's own list is in the backup:\n{old}"
    );
    let now = text(&mut client_b, &on_b.root).await;
    assert!(
        now.contains("shared from a") && !now.contains("b's own"),
        "{now}"
    );

    add(&mut client_a, &on_a.root, "a after bind").await;
    wait_for(&mut client_b, &on_b.root, "a after bind", &logs).await;
    add(&mut client_b, &on_b.root, "b after bind").await;
    let at_a = wait_for(&mut client_a, &on_a.root, "b after bind", &logs).await;
    assert_eq!(
        at_a.matches("shared from a").count(),
        1,
        "no duplicates:\n{at_a}"
    );
}
