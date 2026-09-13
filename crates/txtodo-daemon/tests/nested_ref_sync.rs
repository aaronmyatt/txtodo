//! `test-nested-ref-sync` (plan M5 acceptance, `specs/ref-directories.md` rule 11): syncing a
//! workspace with nested `ref:` directories to a fresh device reproduces the whole tree — same
//! directories at every depth, same `todo.txt` bytes, same ids. Built on the M4 real
//! two-daemon harness (`sync-loopback-converge`), a nested-ref fixture instead of a flat file.
//!
//! **Scope: only the whole-tree-sync half of this task.** The other half of this task's own notes
//! (`todo.sh -d <ref>/todo.cfg ls` agreeing with `txtodo sub`) belongs to `cli-ref-commands` and is
//! deliberately not touched here, per this session's own brief.
//!
//! **Fixture, per the task notes' own shape**: parent (root `todo.txt`) → `child/` (`todo.txt`,
//! `notes.md`) → `child/grandchild/` (`todo.txt`). Copied inline rather than shared
//! (constitution §7, and the task notes' own "do not create a shared fixture module").
//!
//! **`notes.md` is deliberately excluded from the convergence assertions — a real, pre-existing gap,
//! not this test cutting a corner.** Two independent reasons, traced while writing this test:
//! 1. A `notes.md` written directly to disk (as this fixture does, and as any tool other than
//!    txtodo's own `EditNotes` RPC would) never becomes an `Op` at all — `NotesActor`/
//!    `NotesRegistry` are opened lazily, only on a `GetNotes`/`EditNotes` call
//!    (`crates/txtodo-daemon/src/notes_registry.rs`), and nothing at startup diffs a pre-existing
//!    on-disk `notes.md` into a seed op the way `FileActor::recover` does for `todo.txt`.
//!    Device A itself has nothing to transmit for it, regardless of LAN sync.
//! 2. Even if it did: `Workspace::register()` (`crates/txtodo-daemon/src/workspace.rs`) refuses to
//!    build an actor for a notes document, returning `Ok(false)` with no error — so
//!    `lan_apply.rs`'s `get_or_create_actor`, called for an incoming op routed to a `notes.md` path
//!    on a fresh receiving device, would find no actor after a successful-looking `register()` call
//!    and silently drop that op's commit (no warning logged; the peer would just never see an ack).
//!
//! `child/notes.md` is still included in the on-disk fixture (matching the task notes' shape and
//! proving the walk itself does not choke on it — `walker::is_notes_document` still means the file
//! at least gets *seen*), but this test does not assert it reaches device B: asserting either
//! outcome ("it converges" or "it never will") would misrepresent an unfixed gap as this test's own
//! finding. Flagged to the human as a real, separate gap in the notes-sync pipeline, unrelated to
//! `sync-lan-transport`'s own wiring.
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::print_stderr)]
#![cfg(unix)]

mod support;

use std::time::{Duration, Instant};
use support::Daemon;
use txtodo_model::{TaskId, Ulid};

/// Generous, and not this test's own acceptance number — the task notes state no ms/s budget for
/// nested-ref sync (unlike `sync-loopback-converge`'s 2 s or `sync-bench-m4`'s 500 ms); every real
/// convergence measured on this machine so far (`lan_loopback_converge.rs`, `lan_sync_bench.rs`)
/// has been low single-digit milliseconds, so this is headroom, not a target.
const CONVERGE_DEADLINE: Duration = Duration::from_secs(5);

fn rand_u128() -> u128 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    Instant::now().hash(&mut hasher);
    std::process::id().hash(&mut hasher);
    (u128::from(hasher.finish()) << 64) | u128::from(hasher.finish().wrapping_add(1))
}

fn task_id(n: u128) -> TaskId {
    TaskId::new(Ulid::from_u128(0x0C00_0000 + n))
}

/// Parent → child → grandchild, `notes.md` in the middle level, every task line pre-tagged with a
/// valid `id:` so tagged-mode adoption mints nothing extra (no rewrite on adopt, same reasoning
/// `lan_sync_bench.rs`'s fixture doc gives).
fn nested_ref_fixture() -> Vec<(&'static str, String)> {
    vec![
        (
            "todo.txt",
            format!("Parent task ref:child id:{}\n", task_id(1)),
        ),
        (
            "child/todo.txt",
            format!(
                "Child task ref:grandchild id:{}\nx 2026-09-10 2026-09-01 Child done task id:{}\n",
                task_id(2),
                task_id(3)
            ),
        ),
        (
            "child/notes.md",
            "# Child notes\n\nWritten straight to disk, predating any EditNotes call.\n".to_owned(),
        ),
        (
            "child/grandchild/todo.txt",
            format!("Grandchild task id:{}\n", task_id(4)),
        ),
    ]
}

/// Pairs two already-started, same-group daemons through the test-only seam — same pattern as
/// `lan_loopback_converge.rs`'s `pair()`.
async fn pair(a: &mut Daemon, b: &mut Daemon, group_id: u128) {
    let key_hex = "ef".repeat(32);
    a.debug_set_group_key(&group_id.to_string(), &key_hex).await;
    b.debug_set_group_key(&group_id.to_string(), &key_hex).await;
}

/// Polls `b`'s on-disk bytes for `rel` against `want` until they match or `deadline` passes. Reads
/// from disk, not `GetFile` — this test's own claim is about the fresh device's real directory
/// tree, the same thing the task notes assert ("the fresh device must end up with the same
/// directories... and bytes"), not just the daemon's in-memory view of one file.
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

#[tokio::test]
async fn fresh_device_reproduces_the_whole_nested_ref_tree() {
    let group_id = rand_u128();
    let fixture = nested_ref_fixture();
    let files: Vec<(&str, &str)> = fixture.iter().map(|(p, c)| (*p, c.as_str())).collect();
    let mut a = Daemon::start_with_seeded_group_tree(&files, "tagged", group_id).await;
    let mut b = Daemon::start_with_seeded_group("", "tagged", group_id).await;
    pair(&mut a, &mut b, group_id).await;

    // What A actually holds on disk after adoption — not the literal input strings — is the truth
    // B is compared against, same reasoning `wait_for_convergence` uses elsewhere in this crate.
    let want: Vec<(&str, String)> = fixture
        .iter()
        .filter(|(path, _)| *path != "child/notes.md")
        .map(|(path, _)| (*path, a.disk_file(path)))
        .collect();

    let deadline = Instant::now() + CONVERGE_DEADLINE;
    let start = Instant::now();
    for (path, text) in &want {
        wait_for_file(&b, path, text, deadline).await;
    }
    eprintln!(
        "test-nested-ref-sync: whole tree ({} files) converged in {:?}",
        want.len(),
        start.elapsed()
    );

    // Directory nesting itself: grandchild's file only exists at all if `create_dir_all` built
    // both `child/` and `child/grandchild/` on a device that started with neither.
    assert_eq!(
        b.disk_file("child/grandchild/todo.txt"),
        a.disk_file("child/grandchild/todo.txt"),
        "grandchild-depth file must reach a device that started with none of this tree"
    );
}
