//! `#[ignore]`d simulated-reboot proof for task `daemon-always-available`, item 7's second half
//! ("simulated-reboot case: service installed, process killed, recovers via the installed unit
//! rather than triggering another ad-hoc spawn"). Same convention this repo already uses for a
//! real-OS-infra gap it can't close in a sandbox — see `crates/txtodo-daemon/tests/idle_rss.rs`
//! and `tests/lan_sync_bench.rs`'s own doc comments for the precedent this follows.
//!
//! **Why this can't run unattended here**: [`ensure_daemon`]'s best-effort persistent-service
//! install (`spawn.rs::install_persistent_service_best_effort`) calls `service::install` +
//! `service::start`, and `start` on macOS shells out to `launchctl bootstrap`/`kickstart` against
//! the real `gui/<uid>` domain. This sandbox has already demonstrated (every real-daemon
//! integration test in this workspace that exercises the best-effort install path prints it)
//! that `launchctl bootstrap` fails here with `Bootstrap failed: 5: Input/output error` — there is
//! no real GUI login session for `launchctl` to bootstrap an agent into. Without a real,
//! functioning launchd (or systemd --user) session actually supervising the process, "kill it and
//! confirm the *service manager* restarts it" cannot be asserted at all: on this sandbox the
//! process just stays dead, correctly, because no supervisor exists to bring it back — that is
//! the expected, sandboxed behavior, not a bug in `ensure_daemon`.
//!
//! A human (or a real CI runner with an actual login/user session — GitHub Actions' `macos-*`
//! runners have one, confirmed indirectly by this repo's own `release.yml` successfully running
//! `codesign`/GUI-adjacent tooling there) can run this ignored test directly
//! (`cargo test -p txtodo-daemon-launch --test simulated_reboot -- --ignored --nocapture`) to get
//! a real answer. What it checks, once real service supervision is available:
//! 1. `ensure_daemon` ad-hoc-spawns a fresh global daemon (no service installed yet).
//! 2. The best-effort install step left a real, loaded launchd/systemd unit behind (checked via
//!    `service::render`'s own path — the file must exist).
//! 3. Killing the spawned process directly (`SIGKILL`, simulating an abrupt process death that
//!    survives a reboot the same way losing power would) and waiting past the service manager's
//!    own restart backoff should bring a new daemon back up on the same socket, WITHOUT this test
//!    calling `ensure_daemon` again — that's the whole point: recovery came from the installed
//!    unit, not from a second ad-hoc spawn.
#![cfg(unix)]

mod support;

use std::time::Duration;
use support::{TXTODOD_BIN, kill, wait_for_pid};
use txtodo_daemon_launch::{LaunchConfig, ensure_daemon};

#[tokio::test]
#[ignore = "needs a real launchd/systemd user session actually supervising the process — this \
            sandbox's `launchctl bootstrap` fails with \"Bootstrap failed: 5: Input/output \
            error\" (no real GUI login session), so there is nothing here that could restart a \
            killed daemon; see this file's own module doc for what a human/real-CI run checks"]
async fn killed_daemon_recovers_via_the_installed_service_not_a_second_ad_hoc_spawn() {
    let state_dir = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
    let socket = state_dir.path().join("txtodod.sock");
    let registry = state_dir.path().join("registry.db");
    let mut cfg = LaunchConfig::new(&socket);
    cfg.daemon_bin = Some(TXTODOD_BIN.clone());
    cfg.extra_env = vec![
        ("TXTODO_SOCKET".to_owned(), socket.display().to_string()),
        (
            "TXTODO_REGISTRY_DB".to_owned(),
            registry.display().to_string(),
        ),
    ];

    ensure_daemon(&cfg)
        .await
        .unwrap_or_else(|e| panic!("initial ensure_daemon: {e}"));
    let pid = wait_for_pid(&state_dir.path().join("txtodod.pid"));

    // Simulate a reboot's abrupt process loss (not a graceful shutdown — a real power loss or
    // OOM kill gives the daemon no chance to clean up either).
    kill(pid);

    // A real service manager would restart the unit within its own backoff window; this
    // sandbox has none, so this assertion is expected to fail here — that's exactly why the
    // test is `#[ignore]`d rather than asserted as a false "pass".
    tokio::time::sleep(Duration::from_secs(5)).await;
    assert!(
        tokio::net::UnixStream::connect(&socket).await.is_ok(),
        "a real installed service should have restarted the daemon on the same socket by now"
    );
}
