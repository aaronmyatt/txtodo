//! `txtodo.toml` syncs between paired devices like `notes.md` (task workspace-layout: "two devices
//! converge on one folder after a change on one"). Two real daemons over loopback LAN, the same
//! harness `nested_ref_sync.rs` uses: a fresh device receives the file at pairing and its layout
//! follows it; a change through the layout RPC on one device reaches the other's file and layout.
#![cfg(unix)]

mod support;

use std::time::{Duration, Instant};
use support::Daemon;
use txtodo_model::{TaskId, Ulid};
use txtodo_proto::v1 as pb;

/// Headroom, not a target (see `nested_ref_sync.rs`); the layout reload adds the watcher's
/// 150 ms debounce on top of the op's own trip.
const CONVERGE_DEADLINE: Duration = Duration::from_secs(8);

fn rand_u128() -> u128 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    Instant::now().hash(&mut hasher);
    std::process::id().hash(&mut hasher);
    (u128::from(hasher.finish()) << 64) | u128::from(hasher.finish().wrapping_add(1))
}

/// Same test-only seam `nested_ref_sync.rs`'s `pair()` uses.
async fn pair(a: &mut Daemon, b: &mut Daemon, group_id: u128) {
    let key_hex = "ef".repeat(32);
    a.debug_set_group_key(&group_id.to_string(), &key_hex).await;
    b.debug_set_group_key(&group_id.to_string(), &key_hex).await;
}

async fn wait_for_file(b: &Daemon, rel: &str, want: &str, deadline: Instant) {
    loop {
        let got = b.disk_file(rel);
        if got == want {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "{rel}: did not converge within {CONVERGE_DEADLINE:?}\nwant={want:?}\ngot={got:?}\n--- b's log ---\n{}",
            b.log_tail()
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

/// The layout in force on `d`, polled: the watcher's reload runs after the file lands.
async fn wait_for_refs_dir(d: &mut Daemon, want: &str, deadline: Instant) {
    loop {
        let info = d
            .workspace_layout(pb::WorkspaceLayoutRequest::default())
            .await;
        if info.refs_dir == want {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "layout in force did not follow the file within {CONVERGE_DEADLINE:?}: refs_dir={:?} note={:?}\n--- log ---\n{}",
            info.refs_dir,
            info.note,
            d.log_tail()
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

#[tokio::test]
async fn the_layout_file_reaches_a_fresh_device_and_a_change_on_one_reaches_the_other() {
    let group_id = rand_u128();
    let workspace_id = rand_u128();
    let todo = format!(
        "One task id:{}\n",
        TaskId::new(Ulid::from_u128(0x0D00_0001))
    );
    let files = [
        ("todo.txt", todo.as_str()),
        ("txtodo.toml", "refs_dir = \"boards\"\n"),
    ];
    let mut a = Daemon::start_with_seeded_group_and_workspace_tree(
        &files,
        "tagged",
        group_id,
        workspace_id,
    )
    .await;
    let mut b =
        Daemon::start_with_seeded_group_and_workspace("", "tagged", group_id, workspace_id).await;
    pair(&mut a, &mut b, group_id).await;

    // At pairing: the hand-written file A started with, seeded at discovery, reaches B, and B's
    // layout follows it (no ref dir sits in B's old place, so the reload applies).
    let deadline = Instant::now() + CONVERGE_DEADLINE;
    wait_for_file(&b, "txtodo.toml", &a.disk_file("txtodo.toml"), deadline).await;
    wait_for_refs_dir(&mut b, "boards", deadline).await;

    // A change on one device: the RPC on A rewrites the file and records it as an op.
    let info = a
        .workspace_layout(pb::WorkspaceLayoutRequest {
            set: true,
            refs_dir: "lists".into(),
            ..pb::WorkspaceLayoutRequest::default()
        })
        .await;
    assert_eq!(info.refs_dir, "lists", "{info:?}");
    let deadline = Instant::now() + CONVERGE_DEADLINE;
    wait_for_file(&b, "txtodo.toml", &a.disk_file("txtodo.toml"), deadline).await;
    wait_for_refs_dir(&mut b, "lists", deadline).await;
    assert!(
        b.disk_file("txtodo.toml").contains("lists"),
        "{}",
        b.disk_file("txtodo.toml")
    );
}
