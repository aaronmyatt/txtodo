//! `notices.rs`'s tests: what a sync commit shows, and what holds it.

use super::*;
use std::sync::Mutex as StdMutex;
use txtodo_core::notice::QUIET_MS;
use txtodo_model::{DeviceId, Hlc, Op, OpId, TaskId, Ulid};
use txtodo_model::{Field, FieldValue, FilePath, OpKind};
use txtodo_store::{Seq, Stored};

// The engine's `state_order_tests` helpers, copied: that module is test-only in another crate now.
fn task(n: u128) -> TaskId {
    TaskId::new(Ulid::from_u128(0x100 + n))
}

fn at(wall: u64, counter: u16, device: u128) -> Hlc {
    Hlc {
        wall_ms: wall,
        counter,
        device: DeviceId::new(Ulid::from_u128(device)),
    }
}

fn op(hlc: Hlc, kind: OpKind) -> Op {
    Op {
        id: OpId::new(Ulid::from_u128(
            u128::from(hlc.wall_ms) << 32 | u128::from(hlc.counter),
        )),
        hlc,
        principal: txtodo_model::Principal::User { device: hlc.device },
        file: FilePath::new("todo.txt").unwrap_or_else(|e| panic!("path: {e}")),
        kind,
    }
}

#[derive(Default)]
struct Recorded(StdMutex<Vec<String>>, bool);

impl Notifier for Arc<Recorded> {
    fn show(&self, title: &str, body: &str) -> Result<(), String> {
        if self.1 {
            return Err("no session".to_owned());
        }
        self.0.lock().unwrap().push(format!("{title} | {body}"));
        Ok(())
    }
}

fn completed(source: Option<&str>) -> Change {
    let done = op(
        at(10, 0, 9),
        OpKind::SetField {
            task: task(1),
            field: Field::Completed,
            value: FieldValue::Bool(true),
        },
    );
    Change {
        path: FilePath::new("todo.txt").unwrap(),
        hash: [0; 32],
        ops: vec![Stored {
            seq: Seq(1),
            op: done,
        }],
        duplicate_groups: 0,
        source: source.map(str::to_owned),
    }
}

fn hub(fail: bool) -> (Arc<Hub>, Arc<Recorded>) {
    let rec = Arc::new(Recorded(StdMutex::default(), fail));
    (Hub::new(Box::new(rec.clone()), None), rec)
}

#[test]
fn a_sync_completion_shows_once_quiet() {
    let (hub, rec) = hub(false);
    assert!(hub.take_at(&completed(Some("sync")), 1_000));
    assert!(
        hub.flush_at(1_000 + QUIET_MS - 1).is_none(),
        "not quiet yet"
    );
    assert!(hub.flush_at(1_000 + QUIET_MS).is_some());
    assert_eq!(
        *rec.0.lock().unwrap(),
        ["A task was completed on another device | in todo.txt"]
    );
}

#[test]
fn only_another_devices_ops_count() {
    let (hub, _) = hub(false);
    assert!(!hub.take_at(&completed(Some("mcp")), 0));
    assert!(!hub.take_at(&completed(None), 0));
}

#[test]
fn a_client_showing_its_own_holds_the_daemons() {
    let (hub, rec) = hub(false);
    let guard = hub.owner();
    hub.take_at(&completed(Some("sync")), 0);
    assert!(hub.flush_at(QUIET_MS).is_none());
    drop(guard);
    hub.take_at(&completed(Some("sync")), QUIET_MS);
    assert!(
        hub.flush_at(2 * QUIET_MS).is_some(),
        "the desktop closed: the daemon shows"
    );
    assert_eq!(rec.0.lock().unwrap().len(), 1);
}

#[test]
fn a_failing_notifier_turns_notices_off() {
    let (hub, _) = hub(true);
    hub.take_at(&completed(Some("sync")), 0);
    assert!(hub.flush_at(QUIET_MS).is_none());
    assert!(hub.off.load(Ordering::SeqCst));
}

#[test]
fn the_off_marker_holds_notices() {
    let dir = tempfile::tempdir().unwrap();
    let rec = Arc::new(Recorded::default());
    let hub = Hub::new(Box::new(rec.clone()), Some(dir.path().to_path_buf()));
    std::fs::write(dir.path().join(OFF_MARKER), "").unwrap();
    hub.take_at(&completed(Some("sync")), 0);
    assert!(hub.flush_at(QUIET_MS).is_none());
    assert!(rec.0.lock().unwrap().is_empty());
}

#[test]
fn the_text_is_passed_as_arguments_never_as_script() {
    let quote = "say \"hi\" & do shell script \"rm\"";
    if let Some((_, args)) = OsNotifier::command("t", quote) {
        assert_eq!(
            args.last().map(String::as_str),
            Some(quote),
            "the body is its own argument"
        );
        assert!(args.iter().filter(|a| a.contains(quote)).count() == 1);
    }
}
