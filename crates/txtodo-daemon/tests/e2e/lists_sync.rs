//! Task `two-kinds-of-workspace` (ADR 0043): named lists between two real global-mode daemons.
//! Two own devices that both make `work` end with one list holding both devices' lines, and a list
//! made on one reaches the other with no accept. A device the human called not their own gets no
//! list at all.
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::print_stderr)]
#![cfg(unix)]

use crate::default_workspace_foreign::{DEADLINE, default_root, list, pair, text};
use crate::support;

use std::path::Path;
use std::time::Instant;
use support::multi::{MultiClient, MultiWorkspaceDaemon};
use txtodo_proto::v1 as pb;

async fn make(client: &mut MultiClient, name: &str) -> pb::WorkspaceInfo {
    let req = pb::ListEnsureRequest { name: name.into() };
    let made = client.list_ensure(req).await.unwrap().into_inner();
    made.workspace.unwrap()
}

/// The list named `name` on `client`, if it has one.
async fn list_named(client: &mut MultiClient, name: &str) -> Vec<pb::WorkspaceInfo> {
    let all = list(client).await.into_iter();
    all.filter(|w| w.kind() == pb::WorkspaceKind::List && w.name == name)
        .collect()
}

/// Waits until the list `name` on `client` holds every needle; returns its root.
async fn wait_for_list_with(
    client: &mut MultiClient,
    name: &str,
    needles: &[&str],
    log: &dyn Fn() -> String,
) -> String {
    let start = Instant::now();
    loop {
        for w in list_named(client, name).await {
            let body = text(client, &w.root).await;
            if needles.iter().all(|n| body.contains(n)) {
                return w.root;
            }
        }
        assert!(
            start.elapsed() < DEADLINE,
            "no list {name} holding {needles:?}\n{}",
            log()
        );
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    }
}

#[tokio::test]
async fn two_own_devices_that_both_make_a_list_converge_and_a_new_one_arrives_unasked() {
    let (dir_a, dir_b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let (a, mut client_a) = MultiWorkspaceDaemon::start_with_args(dir_a, &[]).await;
    let (b, mut client_b) = MultiWorkspaceDaemon::start_with_args(dir_b, &[]).await;
    let logs = || format!("--- A ---\n{}\n--- B ---\n{}", a.log_tail(), b.log_tail());

    // Both make `work` before they meet, each with a line of its own: no offer decides who owns it.
    let (work_a, work_b) = (
        make(&mut client_a, "work").await,
        make(&mut client_b, "work").await,
    );
    assert_eq!(
        work_a.workspace_id, work_b.workspace_id,
        "the name gives the id"
    );
    assert_eq!(work_a.kind(), pb::WorkspaceKind::List);
    std::fs::write(
        Path::new(&work_a.root).join("todo.txt"),
        "only on a id:01M2CZ0000000000000000000A\n",
    )
    .unwrap();
    std::fs::write(
        Path::new(&work_b.root).join("todo.txt"),
        "only on b id:01M2CZ0000000000000000000B\n",
    )
    .unwrap();

    pair(&mut client_a, true, &mut client_b, true).await;

    let both = ["only on a", "only on b"];
    let root_a = wait_for_list_with(&mut client_a, "work", &both, &logs).await;
    let root_b = wait_for_list_with(&mut client_b, "work", &both, &logs).await;
    for (client, root) in [(&mut client_a, root_a), (&mut client_b, root_b)] {
        let body = text(client, &root).await;
        assert_eq!(body.matches("only on a").count(), 1, "{body}\n{}", logs());
        assert_eq!(body.matches("only on b").count(), 1, "{body}\n{}", logs());
        assert_eq!(
            list_named(client, "work").await.len(),
            1,
            "one list, not two"
        );
    }

    // A list made on A after they paired reaches B with no accept, as a list rather than a mirror.
    let scratch = make(&mut client_a, "scratch").await;
    std::fs::write(
        Path::new(&scratch.root).join("todo.txt"),
        "jotted on a id:01M2CZ0000000000000000000C\n",
    )
    .unwrap();
    let root = wait_for_list_with(&mut client_b, "scratch", &["jotted on a"], &logs).await;
    let on_b = list_named(&mut client_b, "scratch").await;
    assert_eq!(on_b.len(), 1);
    assert_eq!(on_b[0].workspace_id, scratch.workspace_id);
    assert!(!on_b[0].is_remote, "a list, not a Remote mirror: {root}");
    assert!(
        Path::new(&root).to_string_lossy().contains("/lists/"),
        "{root}"
    );
}

#[tokio::test]
async fn a_device_that_is_not_own_gets_no_list() {
    let (dir_a, dir_b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let (a, mut client_a) = MultiWorkspaceDaemon::start_with_args(dir_a, &[]).await;
    let (b, mut client_b) = MultiWorkspaceDaemon::start_with_args(dir_b, &[]).await;
    let logs = || format!("--- A ---\n{}\n--- B ---\n{}", a.log_tail(), b.log_tail());
    let work = make(&mut client_a, "work").await;
    std::fs::write(
        Path::new(&work.root).join("todo.txt"),
        "private to a id:01M2CZ0000000000000000000D\n",
    )
    .unwrap();
    let default_a = default_root(&mut client_a).await;
    std::fs::write(
        Path::new(&default_a).join("todo.txt"),
        "default of a id:01M2CZ0000000000000000000E\n",
    )
    .unwrap();

    // B's human says no: not own. A's default still reaches B, as a Remote mirror under its alias.
    pair(&mut client_a, true, &mut client_b, false).await;
    let start = Instant::now();
    loop {
        let all = list(&mut client_b).await;
        let mirrored = all.iter().filter(|w| w.is_remote).collect::<Vec<_>>();
        let mut seen = false;
        for w in mirrored {
            seen |= text(&mut client_b, &w.root).await.contains("default of a");
        }
        if seen {
            break;
        }
        assert!(start.elapsed() < DEADLINE, "sessions never ran\n{}", logs());
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    }

    // Sessions have run and offers have been made; B holds no list of A's (its own default is one),
    // and none of the list's text.
    for w in list(&mut client_b).await {
        assert!(
            w.is_default || w.kind() != pb::WorkspaceKind::List,
            "{w:?}\n{}",
            logs()
        );
        assert!(
            !text(&mut client_b, &w.root).await.contains("private to a"),
            "{}\n{}",
            w.root,
            logs()
        );
    }
}

/// The harness serves every `Daemon`'s `--dir` as a list, so the two-daemon tests keep syncing once
/// a plain folder stops syncing (`$TXTODO_LISTS_DIR`, `support::seed::list_dirs`).
#[tokio::test]
async fn a_harness_daemon_serves_its_workspace_as_a_list() {
    let mut daemon = support::Daemon::start("(A) one\n").await;
    let kinds: Vec<_> = daemon.workspaces().await.iter().map(|w| w.kind()).collect();
    assert_eq!(kinds, vec![pb::WorkspaceKind::List]);
}
