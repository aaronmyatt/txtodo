//! Real-`txtodod` acceptance tests for task `daemon-auto-upgrade`: a live global daemon whose
//! `txtodod.version` says it is older than the client is restarted with the (newer) resolved
//! binary; an equal or newer one, or one the binary could not improve on, is left alone. The
//! "older" daemon is the real current build with its version file rewritten — the file is what
//! `ensure_daemon` reads, and the real daemon then writes its true version back on restart.
#![cfg(unix)]

mod support;

use std::path::Path;
use support::{TXTODOD_BIN, kill, wait_for_pid};
use txtodo_daemon_launch::{Ensured, LaunchConfig, ensure_daemon};

/// A hermetic global-daemon config under `state_dir`, the same shape `ensure_daemon.rs` uses.
fn config(state_dir: &Path) -> LaunchConfig {
    let socket = state_dir.join("txtodod.sock");
    let mut cfg = LaunchConfig::new(&socket);
    cfg.daemon_bin = Some(TXTODOD_BIN.clone());
    cfg.extra_env = vec![
        ("TXTODO_SOCKET".to_owned(), socket.display().to_string()),
        (
            "TXTODO_REGISTRY_DB".to_owned(),
            state_dir.join("registry.db").display().to_string(),
        ),
    ];
    cfg
}

fn version_file(state_dir: &Path) -> String {
    std::fs::read_to_string(state_dir.join("txtodod.version"))
        .unwrap_or_else(|e| panic!("txtodod.version: {e}"))
}

#[tokio::test]
async fn an_older_running_daemon_is_restarted_with_the_newer_binary() {
    let state_dir = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
    let cfg = config(state_dir.path());
    let pid_file = state_dir.path().join("txtodod.pid");
    ensure_daemon(&cfg)
        .await
        .unwrap_or_else(|e| panic!("first ensure_daemon: {e}"));
    let pid_before = wait_for_pid(&pid_file);
    let real = version_file(state_dir.path());
    assert!(!real.trim().is_empty(), "the daemon wrote its version");

    // Pretend the running daemon is an ancient build; the client claims a newer one and the real
    // binary (`--version`) is newer than "0.0.1" too, so the restart is due.
    std::fs::write(state_dir.path().join("txtodod.version"), "0.0.1")
        .unwrap_or_else(|e| panic!("rewrite version: {e}"));
    let outcome = ensure_daemon(&cfg.clone().with_upgrade_to("0.0.2"))
        .await
        .unwrap_or_else(|e| panic!("upgrade ensure_daemon: {e}"));
    let Ensured::Upgraded { from, to } = outcome else {
        panic!("expected Upgraded, got {outcome:?}")
    };
    assert_eq!(from, "0.0.1");
    assert!(to.contains(real.trim()), "to={to} real={real}");

    let pid_after = wait_for_pid(&pid_file);
    assert_ne!(pid_before, pid_after, "a new txtodod process");
    assert_eq!(
        version_file(state_dir.path()),
        real,
        "the new daemon wrote its real version"
    );
    kill(pid_after);
}

#[tokio::test]
async fn a_same_or_newer_daemon_and_an_unimprovable_one_are_left_alone() {
    let state_dir = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
    let cfg = config(state_dir.path());
    let pid_file = state_dir.path().join("txtodod.pid");
    ensure_daemon(&cfg)
        .await
        .unwrap_or_else(|e| panic!("first ensure_daemon: {e}"));
    let pid = wait_for_pid(&pid_file);
    let real = version_file(state_dir.path());

    // Same build: nothing to do.
    let same = ensure_daemon(&cfg.clone().with_upgrade_to(real.trim()))
        .await
        .unwrap_or_else(|e| panic!("same-version ensure_daemon: {e}"));
    assert_eq!(same, Ensured::AlreadyLive);
    // An older client never downgrades a newer daemon.
    std::fs::write(state_dir.path().join("txtodod.version"), "99.0.0")
        .unwrap_or_else(|e| panic!("rewrite version: {e}"));
    let older_client = ensure_daemon(&cfg.clone().with_upgrade_to("0.0.2"))
        .await
        .unwrap_or_else(|e| panic!("older-client ensure_daemon: {e}"));
    assert_eq!(older_client, Ensured::AlreadyLive);
    // A newer client whose binary is not newer than the daemon: a restart would loop, so no.
    let stale_binary = ensure_daemon(&cfg.clone().with_upgrade_to("99.1.0"))
        .await
        .unwrap_or_else(|e| panic!("stale-binary ensure_daemon: {e}"));
    assert_eq!(stale_binary, Ensured::AlreadyLive);

    assert_eq!(
        wait_for_pid(&pid_file),
        pid,
        "the daemon was never restarted"
    );
    kill(pid);
}
