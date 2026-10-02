//! Duplicate lines across two real daemons (ADR 0032, task sync-drift duplicate-flags). Two
//! devices that each adopted the same text minted their own ids for it, the shape a re-mint leaves
//! (a store lost or rebuilt under an unchanged file): once paired, each holds both copies, and
//! `ListConflicts` shows one group on both. Fixing it on one device must fix it on both.
//!
//! Pairing goes through the test-only seam (`debug_set_group_key`), as `lan_loopback_converge.rs`
//! does, with the group and workspace ids seeded before either process starts.
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::print_stderr)]
#![cfg(unix)]

use crate::support;

use std::time::{Duration, Instant};
use support::Daemon;
use txtodo_proto::v1 as pb;

/// Discovery, handshake and a few op round trips; generous for two debug daemons on a busy
/// machine, and a bounded poll, never a sleep-then-assert.
const DEADLINE: Duration = Duration::from_secs(30);

/// One device's view: its bytes and its groups' task ids.
type View = (String, Vec<Vec<String>>);

fn rand_u128(salt: u64) -> u128 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    (Instant::now(), std::process::id(), salt).hash(&mut hasher);
    (u128::from(hasher.finish()) << 64) | u128::from(hasher.finish().wrapping_add(salt))
}

/// Two daemons that each adopted `todo` on their own, in Sidecar mode, paired.
async fn adopted_apart(todo: &str) -> (Daemon, Daemon) {
    let (group, workspace) = (rand_u128(1), rand_u128(2));
    let mut a =
        Daemon::start_with_seeded_group_and_workspace(todo, "sidecar", group, workspace).await;
    let mut b =
        Daemon::start_with_seeded_group_and_workspace(todo, "sidecar", group, workspace).await;
    let key_hex = "ab".repeat(32);
    a.debug_set_group_key(&group.to_string(), &key_hex).await;
    b.debug_set_group_key(&group.to_string(), &key_hex).await;
    (a, b)
}

async fn view(d: &mut Daemon) -> View {
    let bytes = String::from_utf8(d.daemon_bytes().await).unwrap();
    let groups = d
        .duplicate_groups()
        .await
        .into_iter()
        .map(|g: pb::DuplicateGroup| g.tasks.into_iter().map(|t| t.task_id).collect())
        .collect();
    (bytes, groups)
}

/// Polls both devices until `done` holds for each view, or fails at `DEADLINE` with both views.
async fn wait_both(
    a: &mut Daemon,
    b: &mut Daemon,
    label: &str,
    done: impl Fn(&View) -> bool,
) -> (View, View) {
    let start = Instant::now();
    loop {
        let (va, vb) = (view(a).await, view(b).await);
        if done(&va) && done(&vb) {
            eprintln!("duplicate_groups[{label}]: in {:?}", start.elapsed());
            return (va, vb);
        }
        assert!(
            start.elapsed() < DEADLINE,
            "{label}: not within {DEADLINE:?}\na={va:?}\nb={vb:?}\n--- b's log ---\n{}",
            b.log_tail()
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

fn lines(bytes: &str, text: &str) -> usize {
    bytes.lines().filter(|l| *l == text).count()
}

/// The re-mint case: keeping the newest copy on A, with no rejoin first, leaves exactly one copy
/// on both.
#[tokio::test]
async fn keeping_the_newest_copy_on_one_device_leaves_one_copy_on_both() {
    let (mut a, mut b) = adopted_apart("buy milk\n").await;
    let ((_, groups_a), (_, groups_b)) = wait_both(&mut a, &mut b, "both copies", |(bytes, g)| {
        lines(bytes, "buy milk") == 2 && g.len() == 1 && g[0].len() == 2
    })
    .await;
    assert_eq!(
        groups_a, groups_b,
        "one group, the same ids, oldest first, on both"
    );

    let older = groups_a[0][..1].to_vec();
    a.delete_tasks(&older).await;
    wait_both(&mut a, &mut b, "one copy", |(bytes, g)| {
        lines(bytes, "buy milk") == 1 && g.is_empty()
    })
    .await;
}

/// An edit on A that makes the copies differ drops the group on both; a line typed twice on A
/// shows as a group on both.
#[tokio::test]
async fn an_edit_that_makes_copies_differ_drops_the_group_and_a_line_typed_twice_raises_one() {
    let (mut a, mut b) = adopted_apart("buy milk\n").await;
    wait_both(&mut a, &mut b, "both copies", |(_, g)| g.len() == 1).await;

    a.external_write("buy milk\nbuy milk +shop\n");
    a.settle().await;
    let ((bytes_a, _), (bytes_b, _)) = wait_both(&mut a, &mut b, "edited apart", |(bytes, g)| {
        bytes.contains("buy milk +shop") && g.is_empty()
    })
    .await;
    assert_eq!(bytes_a, bytes_b);

    a.external_write("buy milk\nbuy milk +shop\ncall mum\ncall mum\n");
    a.settle().await;
    let ((_, groups_a), (_, groups_b)) = wait_both(&mut a, &mut b, "typed twice", |(bytes, g)| {
        lines(bytes, "call mum") == 2 && g.len() == 1
    })
    .await;
    assert_eq!(groups_a, groups_b);
}
