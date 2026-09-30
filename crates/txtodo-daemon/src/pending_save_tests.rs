//! Task editor-save-lost: an editor's save that the watcher has not reported yet survives a CLI
//! edit and a peer's ops landing first, and merges once the watcher's event comes (or, if it
//! never does, once the file has sat still).

use crate::actor::{ActorConfig, FileActor, SharedStore};
use crate::clock::FakeClock;
use crate::handle::ActorHandle;
use crate::mutation::Mutation;
use crate::stats::Stats;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};
use txtodo_model::Ulid;
use txtodo_model::{DeviceId, FilePath, Hlc, IdentityMode, Op, OpId, OpKind, Principal, TaskId};
use txtodo_store::Store;

const A: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAA";
const E: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAE";

fn device() -> DeviceId {
    DeviceId::new(Ulid::from_u128(7))
}

fn user() -> Principal {
    Principal::User { device: device() }
}

fn cfg(dir: &Path) -> ActorConfig {
    ActorConfig {
        path: FilePath::new("todo.txt").unwrap(),
        disk: dir.join("todo.txt"),
        device: device(),
        stats: Arc::new(Stats::default()),
        identity_mode: IdentityMode::Tagged,
        tree_dirty: Arc::new(crate::tree_dirty::TreeDirty::default()),
        layout: crate::layout_state::SharedLayout::default(),
    }
}

/// An actor on a list holding one line, `cli-1`. Unit tests get no watcher: nothing reports the
/// saves below until a test sends the watcher's message itself.
fn start(dir: &Path) -> ActorHandle {
    std::fs::write(dir.join("todo.txt"), format!("cli-1 id:{A}\n")).unwrap();
    let store: SharedStore = Arc::new(Mutex::new(Store::open(&dir.join("oplog.db")).unwrap()));
    let clock: Arc<dyn crate::clock::Clock> = Arc::new(FakeClock::new(1_000));
    FileActor::open(cfg(dir), store, clock).unwrap().spawn()
}

fn disk(dir: &Path) -> String {
    String::from_utf8(std::fs::read(dir.join("todo.txt")).unwrap()).unwrap()
}

/// An editor's save: a temp file renamed over the list, as most editors do.
fn editor_appends(dir: &Path, line: &str) {
    let text = format!("{}{line}\n", disk(dir));
    let tmp = dir.join(".todo.txt.swp-save");
    std::fs::write(&tmp, text).unwrap();
    std::fs::rename(&tmp, dir.join("todo.txt")).unwrap();
}

async fn add(handle: &ActorHandle, line: &str) {
    handle
        .apply(vec![Mutation::Add { line: line.into() }], user())
        .await
        .unwrap();
}

#[tokio::test]
async fn a_save_then_a_cli_add_keeps_both() {
    let dir = tempfile::tempdir().unwrap();
    let handle = start(dir.path());
    editor_appends(dir.path(), &format!("editor-2 id:{E}"));
    // The CLI's add lands before the watcher reports the save: it must not write over it.
    add(&handle, "cli-3").await;
    assert!(
        disk(dir.path()).contains("editor-2"),
        "{}",
        disk(dir.path())
    );
    // The watcher's event: the save merges under the add, one write.
    watcher_event(&handle).await;
    let text = disk(dir.path());
    let lines: Vec<&str> = text
        .lines()
        .map(|l| l.split(" id:").next().unwrap())
        .collect();
    assert_eq!(lines, vec!["cli-1", "editor-2", "cli-3"], "{text}");
    assert_eq!(handle.get().await.unwrap().bytes, text.as_bytes());
}

fn peer_insert(n: u128, after: &str, line: &str) -> Op {
    let peer = DeviceId::new(Ulid::from_u128(9));
    let task = TaskId::new(Ulid::from_u128(0x5000 + n));
    Op {
        id: OpId::new(Ulid::from_u128(0x6000 + n)),
        hlc: Hlc {
            wall_ms: 900,
            counter: 0,
            device: peer,
        },
        principal: Principal::User { device: peer },
        file: FilePath::new("todo.txt").unwrap(),
        kind: OpKind::Insert {
            task,
            after: Some(TaskId::new(Ulid::parse(after).unwrap())),
            line: format!("{line} id:{task}"),
        },
    }
}

#[tokio::test]
async fn a_save_then_a_peers_ops_keeps_both() {
    let dir = tempfile::tempdir().unwrap();
    let handle = start(dir.path());
    editor_appends(dir.path(), &format!("editor-2 id:{E}"));
    handle
        .sync_import_ops(vec![peer_insert(1, A, "peer-1")])
        .await
        .unwrap();
    assert!(disk(dir.path()).contains("editor-2"));
    watcher_event(&handle).await;
    let text = disk(dir.path());
    assert!(
        text.contains("editor-2") && text.contains("peer-1"),
        "{text}"
    );
    assert_eq!(text.lines().count(), 3, "{text}");
    assert_eq!(handle.get().await.unwrap().bytes, text.as_bytes());
}

#[tokio::test]
async fn a_save_undone_before_the_event_just_gets_our_write() {
    let dir = tempfile::tempdir().unwrap();
    let handle = start(dir.path());
    let before = disk(dir.path());
    editor_appends(dir.path(), &format!("editor-2 id:{E}"));
    add(&handle, "cli-3").await;
    // The editor puts the old text back before the watcher's event.
    std::fs::write(dir.path().join("todo.txt"), &before).unwrap();
    watcher_event(&handle).await;
    let text = disk(dir.path());
    assert!(
        text.contains("cli-3") && !text.contains("editor-2"),
        "{text}"
    );
    assert_eq!(handle.get().await.unwrap().bytes, text.as_bytes());
}

#[tokio::test]
async fn a_save_the_watcher_never_reports_merges_once_it_has_sat_still() {
    let dir = tempfile::tempdir().unwrap();
    let handle = start(dir.path());
    editor_appends(dir.path(), &format!("editor-2 id:{E}"));
    add(&handle, "cli-3").await;
    assert!(!disk(dir.path()).contains("cli-3"), "held, not written");
    // Backdate the save: it has sat still for longer than `pending_save::SETTLED`.
    // https://doc.rust-lang.org/std/fs/struct.File.html#method.set_modified
    let file = std::fs::File::options()
        .write(true)
        .open(dir.path().join("todo.txt"))
        .unwrap();
    file.set_modified(SystemTime::now() - Duration::from_secs(5))
        .unwrap();
    drop(file);
    // Any message will do: the actor merges after handling it.
    handle.get().await.unwrap();
    let text = handle.get().await.unwrap().bytes;
    let text = String::from_utf8(text).unwrap();
    assert!(
        text.contains("editor-2") && text.contains("cli-3"),
        "{text}"
    );
    assert_eq!(disk(dir.path()), text);
}

/// The watcher's message, then a round trip: `external_change` only queues it.
async fn watcher_event(handle: &ActorHandle) {
    handle.external_change().await.unwrap();
    handle.get().await.unwrap();
}

/// The lab's case (lan-converge, seed 830835642): the save adds a line under `cli-1`, and a
/// peer deletes `cli-1` before the merge. The new line has lost its anchor; it must still land.
#[tokio::test]
async fn a_saved_line_whose_anchor_a_peer_deleted_still_lands() {
    let dir = tempfile::tempdir().unwrap();
    let handle = start(dir.path());
    editor_appends(dir.path(), &format!("editor-2 id:{E}"));
    let mut delete = peer_insert(2, A, "unused");
    delete.kind = txtodo_model::set_field(
        TaskId::new(Ulid::parse(A).unwrap()),
        txtodo_model::Field::Deleted,
        txtodo_model::FieldValue::Bool(true),
    )
    .unwrap();
    handle.sync_import_ops(vec![delete]).await.unwrap();
    watcher_event(&handle).await;
    let text = disk(dir.path());
    assert!(
        text.contains("editor-2") && !text.contains("cli-1"),
        "{text}"
    );
    assert_eq!(handle.get().await.unwrap().bytes, text.as_bytes());
}

/// A stop while a write is held (a crash, a restart): the reopened actor merges the save three-way
/// against the base the store kept, instead of reading the held add's line as deleted.
#[tokio::test]
async fn a_restart_while_a_write_is_held_keeps_both() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("todo.txt"), format!("cli-1 id:{A}\n")).unwrap();
    let store: SharedStore = Arc::new(Mutex::new(
        Store::open(&dir.path().join("oplog.db")).unwrap(),
    ));
    let clock: Arc<dyn crate::clock::Clock> = Arc::new(FakeClock::new(1_000));
    let open = || {
        FileActor::open(cfg(dir.path()), Arc::clone(&store), Arc::clone(&clock))
            .unwrap()
            .spawn()
    };
    let handle = open();
    editor_appends(dir.path(), &format!("editor-2 id:{E}"));
    add(&handle, "cli-3").await;
    assert!(!disk(dir.path()).contains("cli-3"), "held");
    drop(handle);
    let reopened = open();
    let text = String::from_utf8(reopened.get().await.unwrap().bytes).unwrap();
    let lines: Vec<&str> = text
        .lines()
        .map(|l| l.split(" id:").next().unwrap())
        .collect();
    assert_eq!(lines, vec!["cli-1", "editor-2", "cli-3"], "{text}");
    assert_eq!(disk(dir.path()), text);
}
