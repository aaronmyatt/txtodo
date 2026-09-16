//! Task `daemon-workspace-session-multiplex` stage 2's own acceptance bar: two real `txtodod`
//! processes, sharing a peer relationship (paired via a seeded shared group, same shape as
//! `relay_converge.rs`'s own two-daemon setup) with **two** workspaces open on each, converging
//! both over what is verifiably *one* shared relay connection rather than two separate ones —
//! the thing stage 1 (protocol + `Session` library only) could not attempt and today's *separate*-
//! connection-per-workspace code (before this stage) could never prove even if it tried, since it
//! never puts more than one workspace on a connection in the first place.
//!
//! Modeled on `file_carrier_converge.rs`'s `two_workspaces_one_device_share_a_file_carrier_
//! without_cross_contamination` and its `tests/support/multi.rs` harness (`MultiWorkspaceDaemon`),
//! reused rather than rebuilt — extended here (`MultiWorkspaceDaemon::start_with_args`,
//! `support::multi::health_at`) to support `--relay`/`--relay-dial-peer`, which the file-carrier
//! test never needed. Unlike that test (one device, two *separate* single-workspace peers), this
//! one is real multi-workspace-per-peer: **both** A and B hold the same two workspaces open,
//! sharing one relay-dialed connection between them.
//!
//! **Proving "one shared connection", not just final convergence** (the module doc's own
//! instruction: convergence alone doesn't distinguish this from two separate connections that
//! each happened to work). `lan_session_dispatch.rs::drive_shared_session` logs
//! `lan_shared_session_started` with a `workspaces` count exactly once per connection, before a
//! single message is exchanged — `wait_for_both_workspaces_over_one_connection` below checks both
//! daemons' JSON logs for that line reporting `workspaces` >= 2, which only a single connection
//! carrying both workspaces could ever produce (two separate connections would each log
//! `workspaces: 1`).
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::print_stderr)]
#![cfg(unix)]

mod support;

use std::path::Path;
use std::time::{Duration, Instant};
use support::multi::{
    MultiClient, MultiWorkspaceDaemon, debug_set_group_key, file_at, health_at, seed_group_id_at,
    seed_workspace_at,
};
use support::relay::parse_relay_node_id;

const RELAY_URL: &str = "https://use1-1.relay.n0.iroh.link";
const RELAY_BIND_DEADLINE: Duration = Duration::from_secs(20);
const CONVERGE_DEADLINE: Duration = Duration::from_secs(30);
const POLL_INTERVAL: Duration = Duration::from_millis(250);

fn rand_u128() -> u128 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    Instant::now().hash(&mut hasher);
    std::process::id().hash(&mut hasher);
    (u128::from(hasher.finish()) << 64) | u128::from(hasher.finish().wrapping_add(1))
}

/// Builds one device's registry (pre-seeded with the shared group id and both workspace ids) plus
/// its two workspace directories (pre-seeded with `ws1_seed`/`ws2_seed`) — returned separately,
/// not bundled into one struct, so a caller can hand `registry` to
/// `MultiWorkspaceDaemon::start_with_args` (which takes ownership of it) while still holding
/// `ws1`/`ws2` for the rest of the test.
fn make_device_dirs(
    ws1_seed: &str,
    ws2_seed: &str,
    group_id: u128,
    ws1_id: u128,
    ws2_id: u128,
) -> (tempfile::TempDir, tempfile::TempDir, tempfile::TempDir) {
    let registry = tempfile::tempdir().expect("registry tempdir");
    let ws1 = tempfile::tempdir().expect("ws1 tempdir");
    let ws2 = tempfile::tempdir().expect("ws2 tempdir");
    std::fs::write(ws1.path().join("todo.txt"), ws1_seed).expect("seed ws1");
    std::fs::write(ws2.path().join("todo.txt"), ws2_seed).expect("seed ws2");
    let registry_db = registry.path().join("registry.db");
    seed_workspace_at(&registry_db, ws1.path(), ws1_id);
    seed_workspace_at(&registry_db, ws2.path(), ws2_id);
    seed_group_id_at(registry.path(), group_id);
    (registry, ws1, ws2)
}

/// Polls `health_at` until `relay.rs::bind` has recorded a bound node id — the multi-workspace
/// counterpart of `relay_converge.rs`'s own `wait_for_relay_node_id`.
async fn wait_for_relay_node_id(
    client: &mut MultiClient,
    ws: &Path,
    daemon: &MultiWorkspaceDaemon,
    label: &str,
) -> String {
    let start = Instant::now();
    loop {
        let outcome = health_at(client, ws).await.relay_last_outcome;
        if let Some(hex) = parse_relay_node_id(&outcome) {
            return hex;
        }
        assert!(
            start.elapsed() < RELAY_BIND_DEADLINE,
            "{label}: relay endpoint never bound within {RELAY_BIND_DEADLINE:?} (last outcome: {outcome:?})\n--- log ---\n{}",
            daemon.log_tail(),
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// `true` once `daemon`'s own JSON log has recorded `lan_shared_session_started` reporting two or
/// more workspaces on one connection — the module doc's "one shared connection" proof.
fn log_shows_a_multiplexed_session(daemon: &MultiWorkspaceDaemon) -> bool {
    let log = daemon.log_tail();
    log.lines().any(|line| {
        line.contains("lan_shared_session_started")
            && !(line.contains("\"workspaces\":0") || line.contains("\"workspaces\":1"))
    })
}

/// Everything one side of the convergence check needs, bundled so `wait_for_both_workspaces_over_
/// one_connection` stays under `maxParams`.
struct Side<'a> {
    daemon: &'a MultiWorkspaceDaemon,
    client: &'a mut MultiClient,
    ws1: &'a Path,
    ws2: &'a Path,
}

/// Polls both workspaces on both sides until all four agree, then asserts the shared-connection
/// log proof landed somewhere along the way.
async fn wait_for_both_workspaces_over_one_connection(a: &mut Side<'_>, b: &mut Side<'_>) {
    let start = Instant::now();
    loop {
        let a1 = file_at(a.client, a.ws1).await;
        let a2 = file_at(a.client, a.ws2).await;
        let b1 = file_at(b.client, b.ws1).await;
        let b2 = file_at(b.client, b.ws2).await;
        if a1 == b1 && a2 == b2 {
            eprintln!(
                "relay-multiplex: both workspaces converged in {:?}",
                start.elapsed()
            );
            break;
        }
        assert!(
            start.elapsed() < CONVERGE_DEADLINE,
            "did not converge within {CONVERGE_DEADLINE:?}\na1={:?} b1={:?}\na2={:?} b2={:?}\n--- a log ---\n{}\n--- b log ---\n{}",
            String::from_utf8_lossy(&a1),
            String::from_utf8_lossy(&b1),
            String::from_utf8_lossy(&a2),
            String::from_utf8_lossy(&b2),
            a.daemon.log_tail(),
            b.daemon.log_tail(),
        );
        tokio::time::sleep(POLL_INTERVAL).await;
    }
    assert!(
        log_shows_a_multiplexed_session(a.daemon) || log_shows_a_multiplexed_session(b.daemon),
        "neither daemon's log recorded a connection carrying 2+ workspaces — convergence \
         happened, but not verifiably over one shared connection\n--- a log ---\n{}\n--- b log ---\n{}",
        a.daemon.log_tail(),
        b.daemon.log_tail(),
    );
}

/// The real acceptance bar: A and B share one peer relationship over the relay transport, each
/// with the *same two* workspaces open — ws1 starts non-empty on A, empty on B; ws2 starts empty
/// on A, non-empty on B, so both workspaces converge in opposite directions, and both directions
/// must complete over the one connection `--relay-dial-peer` establishes.
#[tokio::test]
async fn two_workspaces_share_one_peer_relationship_converge_over_one_relay_connection() {
    let group_id = rand_u128();
    let ws1_id = rand_u128();
    let ws2_id = rand_u128();

    let (a_registry, a_ws1, a_ws2) = make_device_dirs(
        "buy milk id:01M2CZ0000000000000000A1\n",
        "",
        group_id,
        ws1_id,
        ws2_id,
    );
    let (a, mut a_client) = MultiWorkspaceDaemon::start_with_args(
        a_registry,
        &["--relay".into(), RELAY_URL.into(), "--no-lan".into()],
    )
    .await;
    let a_node_id = wait_for_relay_node_id(&mut a_client, a_ws1.path(), &a, "a").await;

    let (b_registry, b_ws1, b_ws2) = make_device_dirs(
        "",
        "buy bread id:01M2CZ0000000000000000B2\n",
        group_id,
        ws1_id,
        ws2_id,
    );
    let (b, mut b_client) = MultiWorkspaceDaemon::start_with_args(
        b_registry,
        &[
            "--relay".into(),
            RELAY_URL.into(),
            "--no-lan".into(),
            "--relay-dial-peer".into(),
            a_node_id,
        ],
    )
    .await;

    // `debug_set_group_key` writes to the device-wide keystore (ADR 0021: one keystore per
    // device, shared by every workspace) — one call per daemon is enough, regardless of which
    // workspace selector names it.
    let key_hex = "ab".repeat(32);
    debug_set_group_key(&mut a_client, group_id, &key_hex, a_ws1.path()).await;
    debug_set_group_key(&mut b_client, group_id, &key_hex, b_ws1.path()).await;

    let mut side_a = Side {
        daemon: &a,
        client: &mut a_client,
        ws1: a_ws1.path(),
        ws2: a_ws2.path(),
    };
    let mut side_b = Side {
        daemon: &b,
        client: &mut b_client,
        ws1: b_ws1.path(),
        ws2: b_ws2.path(),
    };
    wait_for_both_workspaces_over_one_connection(&mut side_a, &mut side_b).await;
}
