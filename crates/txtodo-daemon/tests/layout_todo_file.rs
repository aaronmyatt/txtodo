//! `todo_file` in `txtodo.toml` against a real `txtodod` with its watcher (task `workspace-layout`):
//! a root list with a name the walker would not find on its own is discovered, watched and reconciled
//! like `todo.txt`; its `ref:` dirs sit in `refs_dir`; and a `todo.txt` beside it is just another
//! list whose refs sit beside it.
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::print_stderr)]
#![cfg(unix)]

mod support;

use std::time::{Duration, Instant};
use support::Daemon;

const DEADLINE: Duration = Duration::from_secs(30);

async fn started() -> Daemon {
    Daemon::start_with_seeded_group_tree(
        &[
            ("txtodo.toml", "todo_file = \"work.txt\"\n"),
            ("work.txt", "plan the launch\n"),
            ("todo.txt", "an ordinary list\n"),
        ],
        "sidecar",
        1,
    )
    .await
}

#[tokio::test]
async fn a_custom_root_list_is_discovered_and_a_todo_txt_beside_it_is_an_ordinary_list() {
    let mut daemon = started().await;
    let docs = daemon.documents().await;
    assert!(docs.contains(&"work.txt".to_owned()), "{docs:?}");
    assert!(docs.contains(&"todo.txt".to_owned()), "{docs:?}");
    assert_eq!(daemon.bytes_of("work.txt").await, b"plan the launch\n");

    // The root list's ref dirs are in `tasks/`; the other list's are beside it.
    let root_ref = daemon.ref_dir_of("work.txt", 1, true).await;
    assert!(root_ref.dir.starts_with("tasks/"), "{root_ref:?}");
    let other_ref = daemon.ref_dir_of("todo.txt", 1, true).await;
    assert!(!other_ref.dir.starts_with("tasks/"), "{other_ref:?}");
    assert!(daemon.dir.path().join(&root_ref.dir).is_dir());
    assert!(daemon.dir.path().join(&other_ref.dir).is_dir());
}

#[tokio::test]
async fn an_edit_to_the_custom_root_list_from_outside_reaches_the_daemon() {
    let mut daemon = started().await;
    std::fs::write(
        daemon.dir.path().join("work.txt"),
        "plan the launch\nbook the vet\n",
    )
    .unwrap();
    let start = Instant::now();
    loop {
        let bytes = daemon.bytes_of("work.txt").await;
        if String::from_utf8_lossy(&bytes).contains("book the vet") {
            return;
        }
        assert!(
            start.elapsed() < DEADLINE,
            "the watcher never saw work.txt\n{}",
            daemon.log_tail()
        );
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}
