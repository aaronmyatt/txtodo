//! Plan M3 acceptance: the save patterns of vim (temp + rename with a swap file), VS Code
//! (truncate, then write in two chunks) and `sed -i` (temp + rename) all reconcile to one
//! consistent state with at most one daemon write and no ops with principal User.
// Integration tests are tests: clippy.toml allows unwrap/expect in #[test] fns but not in their helpers.
#![allow(clippy::expect_used, clippy::unwrap_used)]
// The daemon listens on a unix-domain socket (ADR 0010); these scenarios cannot run on Windows.
#![cfg(unix)]

mod support;

use std::io::Write;
use support::{Daemon, kinds};

const TODO: &str =
    "(A) 2026-09-11 buy ducks +farm\nwalk the dog @home\nx 2026-09-11 2026-09-10 call mum @phone\n";

#[tokio::test]
async fn vim_writes_a_temp_beside_the_file_and_renames_over_it() {
    let mut d = Daemon::start(TODO).await;
    let edited = d.disk().replacen("buy ducks", "buy geese", 1);
    let dir = d.dir.path();
    std::fs::write(dir.join(".todo.txt.swp"), b"vim swap").unwrap();
    let tmp = dir.join("4913");
    std::fs::write(&tmp, &edited).unwrap();
    std::fs::rename(&tmp, dir.join("todo.txt")).unwrap();
    std::fs::remove_file(dir.join(".todo.txt.swp")).unwrap();
    let after = d.settle().await;
    assert!(after.contains("buy geese"));
    assert_consistent(&mut d, &after).await;
    assert_eq!(kinds(&d.history().await), vec!["edit_text"]);
}

#[tokio::test]
async fn vscode_truncates_then_writes_in_two_chunks() {
    let mut d = Daemon::start(TODO).await;
    let edited = d.disk().replacen("walk the dog", "walk the cat", 1);
    let (head, tail) = edited.split_at(edited.len() / 2);
    {
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(d.dir.path().join("todo.txt"))
            .unwrap();
        f.write_all(head.as_bytes()).unwrap();
        f.flush().unwrap();
        // Inside the 150 ms debounce: the half-written file must never become ops.
        std::thread::sleep(std::time::Duration::from_millis(50));
        f.write_all(tail.as_bytes()).unwrap();
    }
    let after = d.settle().await;
    assert_eq!(after, edited);
    assert_consistent(&mut d, &after).await;
    assert_eq!(
        kinds(&d.history().await),
        vec!["edit_text"],
        "no ops from the partial write"
    );
}

#[tokio::test]
async fn sed_in_place_renames_a_temp_over_the_file() {
    let mut d = Daemon::start(TODO).await;
    let before = d.disk();
    let out = std::process::Command::new("sed")
        .args(["-i", "", "s/call mum/call dad/", "todo.txt"])
        .current_dir(d.dir.path())
        .output()
        .unwrap();
    if !out.status.success() {
        // GNU sed spells in-place differently; fall back to the same syscall sequence by hand.
        let tmp = d.dir.path().join("sedAbC123");
        std::fs::write(&tmp, before.replacen("call mum", "call dad", 1)).unwrap();
        std::fs::rename(&tmp, d.dir.path().join("todo.txt")).unwrap();
    }
    let after = d.settle().await;
    assert!(after.contains("call dad"), "{after}");
    assert_consistent(&mut d, &after).await;
    assert_eq!(kinds(&d.history().await), vec!["edit_text"]);
}

async fn assert_consistent(d: &mut Daemon, after: &str) {
    let daemon = d.daemon_bytes().await;
    assert_eq!(daemon, after.as_bytes(), "state == disk");
    assert!(d.writes_since().await <= 1, "at most one write");
    let ops = d.history().await;
    assert!(
        ops.iter().all(|o| o.principal.starts_with("external@")),
        "{:?}",
        kinds(&ops)
    );
    assert!(!ops.is_empty(), "the edit was recorded");
}
