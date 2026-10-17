//! The host natively, over a real SQLite file: the commands the desktop's list view sends, what
//! survives a reopen (the log is the truth), and the change events.

use serde_json::{Value, json};
use txtodo_store::Store;

use crate::host::Host;

fn open(dir: &tempfile::TempDir) -> Host {
    let store = Store::open(&dir.path().join("web.db")).unwrap_or_else(|e| panic!("store: {e}"));
    Host::open(store).unwrap_or_else(|e| panic!("host: {e}"))
}

fn call(host: &mut Host, cmd: &str, args: Value) -> Value {
    host.call(cmd, &args)
        .unwrap_or_else(|e| panic!("{cmd}: {e}"))
}

fn text(host: &mut Host) -> String {
    let f = call(host, "get_file", json!({ "path": "todo.txt" }));
    f["text"].as_str().unwrap_or_default().to_owned()
}

fn add(host: &mut Host, line: &str) {
    let m = json!([{ "kind": "add", "line": line }]);
    call(host, "apply", json!({ "path": "todo.txt", "mutations": m }));
}

/// Add stamps today's creation date (ADR 0011), and the list comes back after a reopen.
#[test]
fn an_added_line_has_a_creation_date_and_survives_a_reopen() {
    let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    let mut host = open(&dir);
    let device = host.device();
    assert_eq!(text(&mut host), "");
    add(&mut host, "call mum +home");
    add(&mut host, "(A) buy milk");
    let today = txtodo_engine::local_date::today_local();
    let want = format!("{today} call mum +home\n(A) {today} buy milk\n");
    assert_eq!(text(&mut host), want);
    drop(host);
    let mut again = open(&dir);
    assert_eq!(again.device(), device, "the device id is kept");
    assert_eq!(text(&mut again), want);
}

/// Complete by line (the done line moves to the bottom), then a whole-text save that swaps the
/// two lines back: each id follows its line.
#[test]
fn complete_then_replace_keeps_each_lines_id() {
    let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    let mut host = open(&dir);
    add(&mut host, "a");
    add(&mut host, "b");
    let m = json!([{ "kind": "complete", "task": { "line_number": 1, "task_id": "" }, "today": "2026-10-17" }]);
    call(
        &mut host,
        "apply",
        json!({ "path": "todo.txt", "mutations": m }),
    );
    let done = call(&mut host, "get_file", json!({ "path": "todo.txt" }));
    let done_text = done["text"].as_str().unwrap_or_default().to_owned();
    let lines: Vec<&str> = done_text.lines().collect();
    assert!(
        lines[1].starts_with("x 2026-10-17 ") && lines[1].ends_with(" a"),
        "{done_text}"
    );
    let ids = done["task_ids"].clone();
    let swapped = format!("{}\n{}\n", lines[1], lines[0]);
    let m = json!([{ "kind": "replace", "base_hash": done["hash"], "contents": swapped }]);
    call(
        &mut host,
        "apply",
        json!({ "path": "todo.txt", "mutations": m }),
    );
    let after = call(&mut host, "get_file", json!({ "path": "todo.txt" }));
    assert_eq!(after["text"], swapped);
    assert_eq!(after["task_ids"], json!([ids[1], ids[0]]));
}

/// A stale base is refused and changes nothing; so is a Replace mixed into a batch.
#[test]
fn a_stale_replace_is_refused() {
    let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    let mut host = open(&dir);
    add(&mut host, "a");
    let m = json!([{ "kind": "replace", "base_hash": "00", "contents": "b\n" }]);
    let err = host.call("apply", &json!({ "path": "todo.txt", "mutations": m }));
    assert!(err.is_err());
    assert!(text(&mut host).ends_with(" a\n"));
}

/// list_files counts tasks; history is newest first and filters by task; apply queues a change.
#[test]
fn list_files_history_and_change_events() {
    let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    let mut host = open(&dir);
    add(&mut host, "a");
    add(&mut host, "x 2026-10-01 b");
    let files = call(&mut host, "list_files", Value::Null);
    assert_eq!(files[0]["path"], "todo.txt");
    assert_eq!(
        (files[0]["done"].clone(), files[0]["total"].clone()),
        (json!(1), json!(2))
    );
    let changes = host.take_changes();
    assert_eq!(changes.len(), 2);
    assert_eq!(changes[1].hash, files[0]["hash"]);
    assert!(host.take_changes().is_empty(), "taken once");
    let ids = call(&mut host, "get_file", json!({ "path": "todo.txt" }))["task_ids"].clone();
    let h = call(
        &mut host,
        "history",
        json!({ "path": "todo.txt", "taskId": ids[0], "limit": 10 }),
    );
    let ops = h["ops"].as_array().cloned().unwrap_or_default();
    assert!(!ops.is_empty() && ops.iter().all(|o| o["task_id"] == ids[0]));
    let seqs: Vec<i64> = ops.iter().filter_map(|o| o["seq"].as_i64()).collect();
    assert!(
        seqs.windows(2).all(|w| w[0] > w[1]),
        "newest first: {seqs:?}"
    );
}

/// A second list opens on first use and is listed from then on, across a reopen.
#[test]
fn a_sub_list_is_listed_once_used() {
    let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    let mut host = open(&dir);
    let m = json!([{ "kind": "add", "line": "pack" }]);
    call(
        &mut host,
        "apply",
        json!({ "path": "tasks/trip/todo.txt", "mutations": m }),
    );
    drop(host);
    let mut again = open(&dir);
    let files = call(&mut again, "list_files", Value::Null);
    let paths: Vec<&str> = files
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|f| f["path"].as_str())
        .collect();
    assert_eq!(paths, ["todo.txt", "tasks/trip/todo.txt"]);
    assert!(
        again.call("get_notes", &Value::Null).is_err(),
        "not answered here"
    );
}
