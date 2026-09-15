//! Plan M8 `relay-converge-test` item 5/6/7: two REAL `txtodod` processes share one temp dir via
//! `--sync-dir`, with LAN and relay both off (`--no-lan`, `--relay` omitted — "networking
//! disabled" per `tasks/relay-converge-test/notes.md`), converging entirely through
//! `crates/txtodo-daemon/src/file_carrier.rs`'s wiring of `txtodo_sync::FileCarrier` — that
//! module's own doc explains why this is new wiring, not just a new test: `carrier.rs`'s module
//! doc deliberately left "importing decoded ops into the store" as a seam for a daemon-level pass,
//! and nothing before this task ever opened a `FileCarrier` from `txtodod` at all
//! (`--sync-dir` has existed in `txtodo-cli`'s config since `sync-file-carrier`, but unused by the
//! daemon).
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::print_stderr)]
#![cfg(unix)]

mod support;

use std::time::{Duration, Instant};
use support::Daemon;
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

/// Every `.ops` file's leading (device-id) segment, deduplicated — the "own-file-only" acceptance
/// criterion made concrete: if either daemon had ever written to the *other's* file, `carrier.rs`'s
/// own `ForeignDevice` refusal would have fired (surfaced here only indirectly, as a convergence
/// failure); what this function checks directly is that exactly two distinct device files exist,
/// one per participant, which is what "each daemon appends only to its own file" actually produces
/// on disk.
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
    // Both daemons must agree on one `workspace_id` (task `daemon-workspace-identity-agreement`)
    // for the AEAD binding to let their sealed batches open at all — two independently-`--dir`-
    // started daemons would otherwise each mint their own, unrelated id, which is exactly the
    // disagreement that binding now correctly refuses to sync across. Real agreement is an
    // offer/accept exchange over the always-on control channel; this sandbox has no real relay to
    // drive that over from a test, so `start_with_seeded_group_and_workspace_args` pre-seeds both
    // sides' own registries with the same id, the same stand-in `seed_group_id` already is for a
    // real pairing ceremony.
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
