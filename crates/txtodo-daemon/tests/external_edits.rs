//! Plan M3 acceptance: the eight external-edit scenarios against a real `txtodod` on a temp dir.
//! After each: the daemon's bytes equal the disk bytes, at most one write happened, unrelated
//! lines are byte-identical, and History shows the expected External op kinds.
// Integration tests are tests: clippy.toml allows unwrap/expect in #[test] fns but not in their helpers.
#![allow(clippy::expect_used, clippy::unwrap_used)]
// The daemon listens on a unix-domain socket (ADR 0010); these scenarios cannot run on Windows.
#![cfg(unix)]

mod support;

use support::{Daemon, kinds, lines_with_ids};

const TODO: &str = "(A) 2026-09-11 buy ducks +farm\n\nwalk the dog @home\nx 2026-09-11 2026-09-10 call mum @phone\n";

fn strip_id(line: &str) -> String {
    line.split(" id:").next().unwrap_or(line).to_owned()
}

/// Every line of `after` except the changed ones must be byte-identical to `before`.
fn unrelated_identical(before: &str, after: &str, changed: &[usize]) {
    for (i, (b, a)) in before.lines().zip(after.lines()).enumerate() {
        if !changed.contains(&i) {
            assert_eq!(b, a, "line {} changed unexpectedly", i + 1);
        }
    }
}

#[tokio::test]
async fn edit_description_in_place() {
    let mut d = Daemon::start(TODO).await;
    let before = d.disk();
    let edited = before.replacen("buy ducks", "buy 400 ducks", 1);
    d.external_write(&edited);
    let after = d.settle().await;
    assert_eq!(after, edited, "no write-back needed: ids were present");
    assert_eq!(d.writes_since().await, 0);
    unrelated_identical(&before, &after, &[0]);
    assert_eq!(kinds(&d.history().await), vec!["edit_text"]);
}

#[tokio::test]
async fn insert_a_line_in_the_middle_gets_an_id_in_one_write() {
    let mut d = Daemon::start(TODO).await;
    let before = d.disk();
    let mut lines = lines_with_ids(&before);
    lines.insert(2, "new one +farm".to_owned());
    d.external_write(&(lines.join("\n") + "\n"));
    let after = d.settle().await;
    assert_eq!(d.writes_since().await, 1, "exactly one write: the id stamp");
    assert!(
        after
            .lines()
            .nth(2)
            .is_some_and(|l| l.starts_with("new one +farm id:")),
        "{after}"
    );
    unrelated_identical(
        &before,
        &after
            .lines()
            .filter(|l| !l.starts_with("new one"))
            .collect::<Vec<_>>()
            .join("\n"),
        &[],
    );
    assert_eq!(kinds(&d.history().await), vec!["insert"]);
}

#[tokio::test]
async fn delete_a_line() {
    let mut d = Daemon::start(TODO).await;
    let before = d.disk();
    let kept: Vec<&str> = before
        .lines()
        .enumerate()
        .filter(|(i, _)| *i != 2)
        .map(|(_, l)| l)
        .collect();
    d.external_write(&(kept.join("\n") + "\n"));
    let after = d.settle().await;
    assert_eq!(after.lines().count(), 3);
    assert_eq!(d.writes_since().await, 0);
    assert_eq!(
        kinds(&d.history().await),
        vec!["set_field"],
        "Deleted = true"
    );
}

#[tokio::test]
async fn reorder_two_lines() {
    let mut d = Daemon::start(TODO).await;
    let before = d.disk();
    let mut lines = lines_with_ids(&before);
    lines.swap(0, 2);
    d.external_write(&(lines.join("\n") + "\n"));
    let after = d.settle().await;
    assert_eq!(after, lines.join("\n") + "\n");
    assert_eq!(d.writes_since().await, 0);
    let k = kinds(&d.history().await);
    assert!(
        k.iter()
            .all(|k| k == "move" || k == "blank_remove" || k == "blank_insert"),
        "{k:?}"
    );
}

#[tokio::test]
async fn strip_every_id_restores_them_and_changes_nothing_else() {
    let mut d = Daemon::start(TODO).await;
    let before = d.disk();
    let stripped: Vec<String> = before.lines().map(strip_id).collect();
    d.external_write(&(stripped.join("\n") + "\n"));
    let after = d.settle().await;
    assert_eq!(after, before, "the same ids come back, nothing else moves");
    assert_eq!(d.writes_since().await, 1);
    assert!(
        d.history().await.is_empty(),
        "recovering ids is not a change"
    );
}

#[tokio::test]
async fn append_a_line_without_an_id() {
    let mut d = Daemon::start(TODO).await;
    let before = d.disk();
    d.external_write(&(before.clone() + "typed in vim\n"));
    let after = d.settle().await;
    assert_eq!(d.writes_since().await, 1);
    assert!(
        after
            .lines()
            .last()
            .is_some_and(|l| l.starts_with("typed in vim id:")),
        "{after}"
    );
    unrelated_identical(&before, &after, &[]);
    assert_eq!(kinds(&d.history().await), vec!["insert"]);
}

#[tokio::test]
async fn same_content_with_crlf_is_not_a_change() {
    let mut d = Daemon::start(TODO).await;
    let before = d.disk();
    d.external_write(&before.replace('\n', "\r\n"));
    let after = d.settle().await;
    assert_eq!(
        after,
        before.replace('\n', "\r\n"),
        "the daemon does not fight the ending"
    );
    assert_eq!(d.writes_since().await, 0);
    assert!(d.history().await.is_empty());
}

#[tokio::test]
async fn todo_sh_do_marks_done_in_place() {
    // `-a`: the vendored todo.sh's own auto-archive (a separate done.txt, its long-standing
    // convention) is not this app's model any more — a completed task stays in todo.txt.
    let mut d = Daemon::start(TODO).await;
    let out = d.todo_sh(&["-a", "do", "3"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let after = d.settle().await;
    assert!(
        after.contains("x ") && after.contains("walk the dog"),
        "marked done in place: {after}"
    );
    assert!(
        d.disk_file("done.txt").is_empty(),
        "no separate done.txt is created"
    );
    let k = kinds(&d.history().await);
    assert!(k.contains(&"set_field".to_owned()), "{k:?}");
}
