//! `write_defer.rs`: a peer batch writes each file once, at its last run, and nothing is lost to
//! a write that waits: not on a crash, not to a message that reads the file, not to an editor save.

use std::sync::Arc;

use txtodo_model::{DeviceId, FilePath, Hlc, Op, OpId, OpKind, Principal, TaskId, Ulid};

use crate::clock::FakeClock;
use crate::lan_apply::commit_incoming_ops;
use crate::lan_session::read;
use crate::lan_session_tests::make_workspace;
use crate::sync_ops_tests::{open, store};

/// Peer `n`'s insert of `name` at the top of `file`.
fn insert(n: u128, file: &str, name: &str) -> Op {
    let device = DeviceId::new(Ulid::from_u128(9_999));
    let task = TaskId::new(Ulid::from_u128(1_000 + n));
    Op {
        id: OpId::new(Ulid::from_u128(5_000 + n)),
        hlc: Hlc {
            wall_ms: 2_000 + u64::try_from(n).unwrap_or(0),
            counter: 0,
            device,
        },
        principal: Principal::User { device },
        file: FilePath::new(file).unwrap_or_else(|e| panic!("{e}")),
        kind: OpKind::Insert {
            task,
            after: None,
            line: format!("{name} id:{task}"),
        },
    }
}

fn disk(dir: &std::path::Path, file: &str) -> String {
    String::from_utf8(std::fs::read(dir.join(file)).unwrap_or_default()).unwrap_or_default()
}

#[tokio::test(flavor = "multi_thread")]
async fn a_batch_writes_each_file_once_at_its_last_run() {
    let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
    let (ws, ..) = make_workspace(dir.path(), [7u8; 32]);
    let sub = "tasks/s/todo.txt";
    let ops = vec![
        insert(1, "todo.txt", "a"),
        insert(2, sub, "b"),
        insert(3, "todo.txt", "c"),
    ];
    let writes = || read(&ws).stats().read().0;
    let before = writes();
    let rt = tokio::runtime::Handle::current();
    let landed = {
        let ws = Arc::clone(&ws);
        tokio::task::spawn_blocking(move || commit_incoming_ops(&ws, &rt, ops))
            .await
            .unwrap_or_else(|e| panic!("join: {e}"))
    };
    assert_eq!(landed.ops, 3);
    assert_eq!(writes() - before, 2, "one write per file, not one per run");
    let root = disk(dir.path(), "todo.txt");
    assert!(root.contains("a id:") && root.contains("c id:"), "{root}");
    assert!(disk(dir.path(), sub).contains("b id:"));
}

#[test]
fn a_crash_with_a_write_owed_finishes_it_at_open() {
    let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
    let store = store(dir.path());
    let clock = Arc::new(FakeClock::new(1_000));
    let mut actor = open(dir.path(), &store, &clock);
    // Two runs owe the write: the second commit's `prev_hash` must name the bytes on disk, not
    // the first run's projection.
    for (n, name) in [(1, "a"), (2, "b")] {
        actor
            .on_sync_run(vec![insert(n, "todo.txt", name)], true)
            .unwrap_or_else(|e| panic!("{e}"));
    }
    assert!(
        !disk(dir.path(), "todo.txt").contains("a id:"),
        "not written yet"
    );
    drop(actor);

    let reopened = open(dir.path(), &store, &clock);
    let text = disk(dir.path(), "todo.txt");
    assert!(text.contains("a id:") && text.contains("b id:"), "{text}");
    assert_eq!(disk(dir.path(), "todo.txt").as_bytes(), reopened.projection);
}

#[tokio::test]
async fn any_other_message_writes_what_is_owed_first() {
    let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
    let store = store(dir.path());
    let clock = Arc::new(FakeClock::new(1_000));
    let handle = open(dir.path(), &store, &clock).spawn();
    handle
        .sync_import_run(vec![insert(1, "todo.txt", "a")], true)
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    assert!(!disk(dir.path(), "todo.txt").contains("a id:"));
    let got = handle.get().await.unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(disk(dir.path(), "todo.txt").as_bytes(), got.bytes);
}

#[tokio::test]
async fn an_editor_save_while_a_write_is_owed_is_merged_not_lost() {
    let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
    let store = store(dir.path());
    let clock = Arc::new(FakeClock::new(1_000));
    let handle = open(dir.path(), &store, &clock).spawn();
    handle
        .sync_import_run(vec![insert(1, "todo.txt", "from a peer")], true)
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    let mine = TaskId::new(Ulid::from_u128(77));
    std::fs::write(
        dir.path().join("todo.txt"),
        format!("typed here id:{mine}\n"),
    )
    .unwrap_or_else(|e| panic!("{e}"));
    handle
        .external_change()
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    let got = handle.get().await.unwrap_or_else(|e| panic!("{e}"));
    let text = String::from_utf8_lossy(&got.bytes).into_owned();
    assert!(
        text.contains("from a peer") && text.contains("typed here"),
        "{text}"
    );
    assert_eq!(disk(dir.path(), "todo.txt"), text, "one write of the merge");
}
