//! Plan M8 `relay-converge-test` item 5/6/7: two REAL `txtodod` processes share one temp dir via
//! `--sync-dir`, with LAN and relay both off, converging entirely through
//! `crates/txtodo-daemon/src/file_carrier.rs`'s wiring of `txtodo_sync::FileCarrier` — see that
//! module's own doc for why this is new wiring, not just a new test.
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::print_stderr)]
#![cfg(unix)]

mod support;

use std::path::Path;
use std::time::{Duration, Instant};
use support::Daemon;
use support::multi::{
    MultiClient, MultiWorkspaceDaemon, debug_set_group_key, file_at, seed_group_id_at,
    seed_workspace_at,
};
use support::relay::{start_with_seeded_group_and_workspace_args, start_with_seeded_group_args};

const CONVERGE_DEADLINE: Duration = Duration::from_secs(30);
const POLL_INTERVAL: Duration = Duration::from_millis(250);

fn rand_u128() -> u128 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    Instant::now().hash(&mut hasher);
    std::process::id().hash(&mut hasher);
    (u128::from(hasher.finish()) << 64) | u128::from(hasher.finish().wrapping_add(1))
}

async fn wait_for_convergence(from: &mut Daemon, to: &mut Daemon, label: &str) {
    let want = from.daemon_bytes().await;
    let start = Instant::now();
    loop {
        let got = to.daemon_bytes().await;
        if got == want {
            eprintln!(
                "file-carrier-converge[{label}]: converged in {:?}",
                start.elapsed()
            );
            return;
        }
        assert!(
            start.elapsed() < CONVERGE_DEADLINE,
            "{label}: did not converge within {CONVERGE_DEADLINE:?}\nwant={:?}\ngot={:?}\n--- from log ---\n{}\n--- to log ---\n{}",
            String::from_utf8_lossy(&want),
            String::from_utf8_lossy(&got),
            from.log_tail(),
            to.log_tail(),
        );
        tokio::time::sleep(POLL_INTERVAL).await;
    }
}

/// Every `.ops` file's leading (device-id) segment, deduplicated — proves "each daemon appends
/// only to its own file" (`carrier.rs`'s own `ForeignDevice` refusal would fire otherwise).
fn distinct_device_files(sync_dir: &std::path::Path) -> Vec<String> {
    let mut devices: Vec<String> = std::fs::read_dir(sync_dir)
        .expect("read sync dir")
        .filter_map(|e| e.ok())
        .filter_map(|e| e.file_name().to_str().map(str::to_owned))
        .filter_map(|name| name.strip_suffix(".ops").map(str::to_owned))
        .map(|stem| stem.split('-').next().unwrap_or(&stem).to_owned())
        .collect();
    devices.sort();
    devices.dedup();
    devices
}

#[tokio::test]
async fn two_real_daemons_converge_via_file_carrier_with_no_network() {
    let group_id = rand_u128();
    // Both daemons must agree on one `workspace_id` for the AEAD binding to let their sealed
    // batches open at all; `start_with_seeded_group_and_workspace_args` pre-seeds both sides'
    // registries with the same id, standing in for a real pairing/offer-accept ceremony.
    let workspace_id = rand_u128();
    let sync_dir = tempfile::tempdir().expect("sync tempdir");
    let sync_dir_arg = sync_dir.path().to_string_lossy().into_owned();

    let mut a = start_with_seeded_group_and_workspace_args(
        &[("todo.txt", "buy milk id:01M2CZ00000000000000000A\n")],
        "tagged",
        group_id,
        workspace_id,
        &["--no-lan".into(), "--sync-dir".into(), sync_dir_arg.clone()],
    )
    .await;
    let mut b = start_with_seeded_group_and_workspace_args(
        &[("todo.txt", "")],
        "tagged",
        group_id,
        workspace_id,
        &["--no-lan".into(), "--sync-dir".into(), sync_dir_arg],
    )
    .await;

    let key_hex = "ef".repeat(32);
    a.debug_set_group_key(&group_id.to_string(), &key_hex).await;
    b.debug_set_group_key(&group_id.to_string(), &key_hex).await;

    wait_for_convergence(&mut a, &mut b, "a-to-b").await;
    assert_eq!(a.daemon_bytes().await, b.daemon_bytes().await);

    // B -> A too, so both devices have written their own `.ops` file: the "own-file-only"
    // acceptance criterion, made concrete (see `distinct_device_files`'s own doc).
    b.external_write(
        "buy milk id:01M2CZ00000000000000000A\nwalk the dog id:01M2CZ00000000000000000B\n",
    );
    b.settle().await;
    wait_for_convergence(&mut b, &mut a, "b-to-a").await;
    assert_eq!(a.daemon_bytes().await, b.daemon_bytes().await);

    let devices = distinct_device_files(&sync_dir.path().join("sync"));
    assert_eq!(
        devices.len(),
        2,
        "exactly one .ops file per participating device, got {devices:?}"
    );
}

/// A daemon with `--sync-dir` set but no peer writing there yet must not error or hang; the file
/// carrier's send/receive tick simply has nothing to do.
#[tokio::test]
async fn file_carrier_alone_is_a_quiet_no_op() {
    let group_id = rand_u128();
    let sync_dir = tempfile::tempdir().expect("sync tempdir");
    let mut a = start_with_seeded_group_args(
        &[("todo.txt", "buy milk\n")],
        "tagged",
        group_id,
        &[
            "--no-lan".into(),
            "--sync-dir".into(),
            sync_dir.path().to_string_lossy().into_owned(),
        ],
    )
    .await;
    let health = a.health().await;
    assert!(health.watcher_alive);
}

/// Sets up device A (two workspaces sharing `sync_dir`) plus its two single-workspace peers
/// `b1`/`b2`, each already given the matching group key — split out for the line budget.
async fn two_workspace_setup(
    sync_dir: &Path,
) -> (
    MultiWorkspaceDaemon,
    MultiClient,
    tempfile::TempDir,
    tempfile::TempDir,
    Daemon,
    Daemon,
) {
    let registry_dir = tempfile::tempdir().expect("registry tempdir");
    let registry_db = registry_dir.path().join("registry.db");

    let ws1_dir = tempfile::tempdir().expect("ws1 tempdir");
    let ws2_dir = tempfile::tempdir().expect("ws2 tempdir");
    std::fs::write(
        ws1_dir.path().join("todo.txt"),
        "buy milk id:01M2CZ00000000000000000A\n",
    )
    .expect("seed ws1");
    std::fs::write(ws2_dir.path().join("todo.txt"), "").expect("seed ws2");
    let ws1_id = rand_u128();
    let ws2_id = rand_u128();
    seed_workspace_at(&registry_db, ws1_dir.path(), ws1_id);
    seed_workspace_at(&registry_db, ws2_dir.path(), ws2_id);

    // One shared group/key for both workspaces — ADR 0021: every workspace on one device shares
    // the same sync group and keystore, device-wide (`KeyId::Group(epoch)` has no workspace
    // dimension of its own); `workspace_id` alone is what tells the two apart on the wire. Seeded
    // before A starts (see `seed_group_id_at`'s own doc for why that ordering matters).
    let group = rand_u128();
    seed_group_id_at(registry_dir.path(), group);

    let (a, mut a_client) = MultiWorkspaceDaemon::start(registry_dir, sync_dir).await;

    let key_hex = "11".repeat(32);
    debug_set_group_key(&mut a_client, group, &key_hex, ws1_dir.path()).await;

    let sync_dir_arg = sync_dir.to_string_lossy().into_owned();
    let mut b1 = start_with_seeded_group_and_workspace_args(
        &[("todo.txt", "")],
        "tagged",
        group,
        ws1_id,
        &["--no-lan".into(), "--sync-dir".into(), sync_dir_arg.clone()],
    )
    .await;
    let mut b2 = start_with_seeded_group_and_workspace_args(
        &[("todo.txt", "buy milk id:01M2CZ00000000000000000B\n")],
        "tagged",
        group,
        ws2_id,
        &["--no-lan".into(), "--sync-dir".into(), sync_dir_arg],
    )
    .await;
    b1.debug_set_group_key(&group.to_string(), &key_hex).await;
    b2.debug_set_group_key(&group.to_string(), &key_hex).await;
    (a, a_client, ws1_dir, ws2_dir, b1, b2)
}

/// Bundles `wait_for_both`'s per-peer arguments under clippy's 5-argument cap.
struct Peers<'a> {
    ws1: &'a Path,
    ws2: &'a Path,
    b1: &'a mut Daemon,
    b2: &'a mut Daemon,
}

/// Polls `b1`/`b2` against A's two workspaces until both converge or `CONVERGE_DEADLINE` passes.
async fn wait_for_both(
    a: &MultiWorkspaceDaemon,
    a_client: &mut MultiClient,
    peers: &mut Peers<'_>,
) {
    let start = Instant::now();
    loop {
        // Re-fetched every iteration, not captured once: ws2 starts empty on A and non-empty on
        // b2, so it is *A* that must catch up there — unlike ws1's fixed source, A's own content
        // keeps changing until both sides agree, whichever direction that ends up flowing.
        let want1 = file_at(a_client, peers.ws1).await;
        let want2 = file_at(a_client, peers.ws2).await;
        let got1 = peers.b1.daemon_bytes().await;
        let got2 = peers.b2.daemon_bytes().await;
        if got1 == want1 && got2 == want2 {
            eprintln!(
                "file-carrier-converge[multi-workspace]: both converged in {:?}",
                start.elapsed()
            );
            return;
        }
        assert!(
            start.elapsed() < CONVERGE_DEADLINE,
            "did not converge within {CONVERGE_DEADLINE:?}\nwant1={:?} got1={:?}\nwant2={:?} got2={:?}\n--- a log ---\n{}\n--- b1 log ---\n{}\n--- b2 log ---\n{}",
            String::from_utf8_lossy(&want1),
            String::from_utf8_lossy(&got1),
            String::from_utf8_lossy(&want2),
            String::from_utf8_lossy(&got2),
            a.log_tail(),
            peers.b1.log_tail(),
            peers.b2.log_tail(),
        );
        tokio::time::sleep(POLL_INTERVAL).await;
    }
}

/// Task `daemon-shared-sync-link` stage 6: one device (global mode) holds *two* workspaces
/// sharing one `--sync-dir`, each syncing with its own peer — proves the consolidated
/// `DeviceFileCarrier` really does keep them apart by peeked `workspace_id`, not just by luck.
#[tokio::test]
async fn two_workspaces_one_device_share_a_file_carrier_without_cross_contamination() {
    let sync_dir = tempfile::tempdir().expect("sync tempdir");
    let (a, mut a_client, ws1_dir, ws2_dir, mut b1, mut b2) =
        two_workspace_setup(sync_dir.path()).await;

    wait_for_both(
        &a,
        &mut a_client,
        &mut Peers {
            ws1: ws1_dir.path(),
            ws2: ws2_dir.path(),
            b1: &mut b1,
            b2: &mut b2,
        },
    )
    .await;

    // No cross-contamination: workspace 1's peer never sees workspace 2's content or vice versa.
    let b1_bytes = b1.daemon_bytes().await;
    let b2_bytes = b2.daemon_bytes().await;
    assert_ne!(
        b1_bytes, b2_bytes,
        "the two workspaces' content must stay distinct"
    );
    assert!(!String::from_utf8_lossy(&b1_bytes).contains("000000000B"));
    assert!(!String::from_utf8_lossy(&b2_bytes).contains("000000000A"));
}
