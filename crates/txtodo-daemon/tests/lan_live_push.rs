//! Task `sync-live-push`'s acceptance: two real global-mode `txtodod` processes, LAN only, default
//! workspace only (no offer, no mirror — both register it under ADR 0029's reserved id). After the
//! first exchange the one LAN session stays open, and an edit on either side reaches the other by
//! push, with no new session in between: the count of `lan_shared_session_started` lines across
//! both logs does not grow while later edits converge.
//!
//! The harness sets `TXTODO_RESYNC_INTERVAL_MS=1000`, so a redial would show up within a second;
//! the test waits well past that before its later edits. Bounds are generous (this machine is slow
//! in bursts); the measured times are printed.
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::print_stderr)]
#![cfg(unix)]

mod support;

use std::path::Path;
use std::time::{Duration, Instant};
use support::multi::{
    MultiClient, MultiWorkspaceDaemon, debug_set_group_key, file_at, seed_group_id_at,
};

const FIRST_DEADLINE: Duration = Duration::from_secs(60);
const PUSH_DEADLINE: Duration = Duration::from_secs(10);

/// Waits for `client`'s default to hold `needle`; returns how long it took.
async fn wait_for(
    client: &mut MultiClient,
    default: &Path,
    needle: &str,
    bound: Duration,
) -> Duration {
    let start = Instant::now();
    loop {
        let text = String::from_utf8(file_at(client, default).await).unwrap();
        if text.contains(needle) {
            return start.elapsed();
        }
        assert!(start.elapsed() < bound, "{needle} never arrived: {text:?}");
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

fn sessions(a: &MultiWorkspaceDaemon, b: &MultiWorkspaceDaemon) -> usize {
    let started = "lan_shared_session_started";
    a.log_tail().matches(started).count() + b.log_tail().matches(started).count()
}

fn append(default: &Path, line: &str) {
    let path = default.join("todo.txt");
    let mut text = std::fs::read_to_string(&path).unwrap();
    text.push_str(line);
    text.push('\n');
    std::fs::write(path, text).unwrap();
}

#[tokio::test]
async fn an_edit_is_pushed_over_the_open_lan_session_with_no_redial() {
    let group_id = 0x11FE_0005_u128 ^ u128::from(std::process::id());
    let (dir_a, dir_b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    seed_group_id_at(dir_a.path(), group_id);
    seed_group_id_at(dir_b.path(), group_id);
    let (default_a, default_b) = (dir_a.path().join("default"), dir_b.path().join("default"));

    let (a, mut client_a) = MultiWorkspaceDaemon::start_with_args(dir_a, &[]).await;
    let (b, mut client_b) = MultiWorkspaceDaemon::start_with_args(dir_b, &[]).await;
    let key_hex = "ab".repeat(32);
    debug_set_group_key(&mut client_a, group_id, &key_hex, &default_a).await;
    debug_set_group_key(&mut client_b, group_id, &key_hex, &default_b).await;

    // First contact: mDNS, a dial, the Greet/Want exchange.
    append(&default_a, "first id:01M2CZ00000000000000000A");
    let took = wait_for(&mut client_b, &default_b, "first", FIRST_DEADLINE).await;
    eprintln!("sync-live-push: first edit converged in {took:?}");
    let linked = sessions(&a, &b);
    assert!(linked >= 1, "a session ran:\n{}", a.log_tail());

    // Long enough that the old 1 s redial would have opened new sessions by now.
    tokio::time::sleep(Duration::from_secs(3)).await;
    append(&default_a, "second id:01M2CZ00000000000000000B");
    let took = wait_for(&mut client_b, &default_b, "second", PUSH_DEADLINE).await;
    eprintln!("sync-live-push: a -> b pushed in {took:?}");
    append(&default_b, "third id:01M2CZ00000000000000000C");
    let took = wait_for(&mut client_a, &default_a, "third", PUSH_DEADLINE).await;
    eprintln!("sync-live-push: b -> a pushed in {took:?}");

    assert_eq!(
        sessions(&a, &b),
        linked,
        "no new session between edits\n--- A ---\n{}\n--- B ---\n{}",
        a.log_tail(),
        b.log_tail()
    );
}
