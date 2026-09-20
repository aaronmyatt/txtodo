//! Task `default-workspace`: two real `txtodod` processes, each with its own default workspace at a
//! different absolute path, converge to the union of their tasks. Nothing is paired or negotiated
//! for identity: both register the default under one reserved `WorkspaceId`, and sync is keyed by
//! workspace id. Carried over a shared `--sync-dir` (the file carrier), so no network is needed.
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::print_stderr)]
#![cfg(unix)]

mod support;

use std::time::{Duration, Instant};
use support::multi::{MultiWorkspaceDaemon, debug_set_group_key, file_at, seed_group_id_at};

const CONVERGE_DEADLINE: Duration = Duration::from_secs(60);
const POLL_INTERVAL: Duration = Duration::from_millis(250);

#[tokio::test]
async fn two_devices_defaults_at_different_paths_converge_to_the_union() {
    let group_id = 0x0B5E_55ED_u128 ^ u128::from(std::process::id());
    let sync_dir = tempfile::tempdir().expect("sync tempdir");
    let (dir_a, dir_b) = (
        tempfile::tempdir().expect("a state dir"),
        tempfile::tempdir().expect("b state dir"),
    );
    seed_group_id_at(dir_a.path(), group_id);
    seed_group_id_at(dir_b.path(), group_id);
    // Where each daemon will create its default: `<state dir>/default`, beside its socket.
    let (default_a, default_b) = (dir_a.path().join("default"), dir_b.path().join("default"));
    assert_ne!(
        default_a, default_b,
        "the paths need not match, and here they do not"
    );

    let (_a, mut client_a) = MultiWorkspaceDaemon::start(dir_a, sync_dir.path()).await;
    let (_b, mut client_b) = MultiWorkspaceDaemon::start(dir_b, sync_dir.path()).await;
    let key_hex = "ab".repeat(32);
    debug_set_group_key(&mut client_a, group_id, &key_hex, &default_a).await;
    debug_set_group_key(&mut client_b, group_id, &key_hex, &default_b).await;
    assert_eq!(
        file_at(&mut client_a, &default_a).await,
        b"",
        "created empty: no seed task"
    );

    // Each device adds its own task to its own default, straight to its file.
    std::fs::write(
        default_a.join("todo.txt"),
        "buy milk id:01M2CZ00000000000000000A\n",
    )
    .unwrap();
    std::fs::write(
        default_b.join("todo.txt"),
        "walk the dog id:01M2CZ00000000000000000B\n",
    )
    .unwrap();

    let start = Instant::now();
    loop {
        let a = String::from_utf8(file_at(&mut client_a, &default_a).await).unwrap();
        let b = String::from_utf8(file_at(&mut client_b, &default_b).await).unwrap();
        let both = |t: &str| t.contains("buy milk") && t.contains("walk the dog");
        if both(&a) && both(&b) {
            assert_eq!(a.lines().count(), 2, "the union, not duplicates: {a:?}");
            assert_eq!(b.lines().count(), 2, "the union, not duplicates: {b:?}");
            return;
        }
        assert!(
            start.elapsed() < CONVERGE_DEADLINE,
            "defaults did not converge\nA={a:?}\nB={b:?}\n--- A log ---\n{}\n--- B log ---\n{}",
            _a.log_tail(),
            _b.log_tail()
        );
        tokio::time::sleep(POLL_INTERVAL).await;
    }
}
