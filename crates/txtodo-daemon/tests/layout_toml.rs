//! `txtodo.toml` against a real `txtodod` with its watcher (task `workspace-layout`): a workspace
//! with no file keeps ref dirs in `tasks/`; writing the file moves where the next one is made;
//! a change is refused while a ref dir sits in the old place, and the daemon says where it is
//! looking, not what the file wishes.
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::print_stderr)]
#![cfg(unix)]

mod support;

use std::time::{Duration, Instant};
use support::Daemon;

const DEADLINE: Duration = Duration::from_secs(30);

/// Polls `RefDir` (read-only) for line 1 until its directory starts with `prefix`.
async fn wait_for_ref_dir_under(daemon: &mut Daemon, prefix: &str) -> String {
    let start = Instant::now();
    loop {
        let dir = daemon.ref_dir(1, false).await.dir;
        if dir.starts_with(prefix) {
            return dir;
        }
        assert!(
            start.elapsed() < DEADLINE,
            "ref dir never moved under {prefix:?}, still {dir:?}\n{}",
            daemon.log_tail()
        );
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

#[tokio::test]
async fn the_layout_file_moves_where_ref_dirs_are_made_and_refuses_a_change_under_live_dirs() {
    let mut daemon =
        Daemon::start_with_mode("(A) plan the launch\nbook the vet\n", "sidecar").await;

    // No file: the default, `tasks/`.
    assert!(daemon.ref_dir(1, false).await.dir.starts_with("tasks/"));

    // Writing the file moves it. Nothing sits in `tasks/` yet, so the change applies.
    std::fs::write(
        daemon.dir.path().join("txtodo.toml"),
        "refs_dir = \"stuff\"\n",
    )
    .unwrap();
    let dir = wait_for_ref_dir_under(&mut daemon, "stuff/").await;
    let made = daemon.ref_dir(1, true).await;
    assert_eq!(made.dir, dir);
    assert!(
        daemon.dir.path().join(&made.dir).is_dir(),
        "created under stuff/: {made:?}"
    );
    assert!(
        !daemon.dir.path().join("tasks").exists(),
        "nothing was made in tasks/"
    );

    // A ref dir now lives in `stuff/`, so moving the layout again is refused.
    std::fs::write(
        daemon.dir.path().join("txtodo.toml"),
        "refs_dir = \"other\"\n",
    )
    .unwrap();
    tokio::time::sleep(Duration::from_secs(3)).await;
    assert!(
        daemon.ref_dir(1, false).await.dir.starts_with("stuff/"),
        "still stuff/: the refused change did not take"
    );
    assert!(
        daemon.log_tail().contains("layout_reload_kept_last_good"),
        "{}",
        daemon.log_tail()
    );
}
