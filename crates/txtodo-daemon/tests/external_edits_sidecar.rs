//! Sidecar's counterpart to `external_edits.rs`: the same family of external-edit scenarios
//! against a real `txtodod --identity-mode sidecar`, but the file never carries an `id:` tag at
//! any point — identity survives purely via fingerprint re-matching. "Strip every id" doesn't
//! apply here (there's nothing to strip); a full description rewrite replaces it instead, since
//! that is sidecar's own distinctive tradeoff (delete+insert, not a silent id transplant).
// Integration tests are tests: clippy.toml allows unwrap/expect in #[test] fns but not in their helpers.
#![allow(clippy::expect_used, clippy::unwrap_used)]
// The daemon listens on a unix-domain socket (ADR 0010); these scenarios cannot run on Windows.
#![cfg(unix)]

mod support;

use support::{Daemon, kinds};

const TODO: &str = "(A) 2026-09-11 buy ducks +farm\n\nwalk the dog @home\nx 2026-09-11 2026-09-10 call mum @phone\n";

async fn sidecar(todo: &str) -> Daemon {
    Daemon::start_with_mode(todo, "sidecar").await
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
    let mut d = sidecar(TODO).await;
    let before = d.disk();
    assert!(!before.contains("id:"), "sidecar never stamps: {before}");
    let edited = before.replacen("buy ducks", "buy 400 ducks", 1);
    d.external_write(&edited);
    let after = d.settle().await;
    assert_eq!(after, edited, "no write-back needed: content alone matches");
    assert_eq!(d.writes_since().await, 0);
    unrelated_identical(&before, &after, &[0]);
    assert_eq!(kinds(&d.history().await), vec!["edit_text"]);
}

#[tokio::test]
async fn insert_a_line_in_the_middle_needs_no_write_back() {
    let mut d = sidecar(TODO).await;
    let before = d.disk();
    let mut lines: Vec<&str> = before.lines().collect();
    lines.insert(2, "new one +farm");
    let edited = lines.join("\n") + "\n";
    d.external_write(&edited);
    let after = d.settle().await;
    assert_eq!(
        after, edited,
        "sidecar stamps nothing, so the file the user wrote is already right"
    );
    assert_eq!(d.writes_since().await, 0);
    // The new line sits after a blank. An `Insert` anchors on the task above and lands above that
    // task's blank, so the ops a peer can replay are three (task sync-poison-op); the single
    // `insert` this used to record put the line on the wrong side of the blank on a peer.
    assert_eq!(
        kinds(&d.history().await),
        vec!["insert", "blank_insert", "blank_remove"]
    );
}

#[tokio::test]
async fn delete_a_line() {
    let mut d = sidecar(TODO).await;
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
async fn reorder_two_lines_matches_by_content_not_position() {
    let mut d = sidecar(TODO).await;
    let before = d.disk();
    let mut lines: Vec<&str> = before.lines().collect();
    lines.swap(0, 2);
    let edited = lines.join("\n") + "\n";
    d.external_write(&edited);
    let after = d.settle().await;
    assert_eq!(after, edited);
    assert_eq!(d.writes_since().await, 0);
    let k = kinds(&d.history().await);
    assert!(
        k.iter()
            .all(|k| k == "move" || k == "blank_remove" || k == "blank_insert"),
        "{k:?}"
    );
}

#[tokio::test]
async fn a_full_description_rewrite_becomes_delete_plus_insert() {
    let mut d = sidecar(TODO).await;
    let before = d.disk();
    // Rewrite task 1's description entirely: unrelated to "buy ducks +farm" in every term of the
    // cost function, so this must not force-match the old task's identity onto it.
    let edited = before.replacen(
        "(A) 2026-09-11 buy ducks +farm",
        "call the dentist about a checkup",
        1,
    );
    d.external_write(&edited);
    let after = d.settle().await;
    assert_eq!(after, edited);
    let k = kinds(&d.history().await);
    // The deleted task's anchor identity changes underneath the blank right after it, so that
    // blank gets removed and reinserted too (same net position, no visible effect) — a harmless
    // side effect of keying blank runs by the anchor task's identity, not its raw position.
    assert!(
        k.contains(&"set_field".to_owned()) && k.contains(&"insert".to_owned()),
        "a full rewrite is a visible duplicate, not a silent identity transplant: {k:?}"
    );
}

#[tokio::test]
async fn append_a_line_needs_no_write_back() {
    let mut d = sidecar(TODO).await;
    let before = d.disk();
    d.external_write(&(before.clone() + "typed in vim\n"));
    let after = d.settle().await;
    assert_eq!(d.writes_since().await, 0);
    assert_eq!(after.lines().last(), Some("typed in vim"));
    unrelated_identical(&before, &after, &[]);
    assert_eq!(kinds(&d.history().await), vec!["insert"]);
}

#[tokio::test]
async fn same_content_with_crlf_is_not_a_change() {
    let mut d = sidecar(TODO).await;
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
    let mut d = sidecar(TODO).await;
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

/// Task complete-to-bottom: only the `Complete` action moves a line. Typing `x ` in an editor is
/// an edit like any other, so the line stays where the human left it and nothing is written back.
#[tokio::test]
async fn a_hand_completed_line_is_not_moved() {
    let mut d = sidecar(TODO).await;
    let before = d.disk();
    let edited = before.replacen(
        "(A) 2026-09-11 buy ducks",
        "x 2026-09-20 2026-09-11 buy ducks",
        1,
    );
    d.external_write(&edited);
    let after = d.settle().await;
    assert_eq!(after, edited, "the done line stays on line 1");
    assert_eq!(d.writes_since().await, 0, "the daemon wrote nothing back");
    assert!(
        !kinds(&d.history().await).contains(&"move".to_owned()),
        "no move was recorded"
    );
}

/// A done line a human moved somewhere else stays there: completing is one move at that moment,
/// not a rule the daemon keeps true on every write.
#[tokio::test]
async fn a_hand_moved_done_line_stays_where_it_was_put() {
    let mut d = sidecar(TODO).await;
    let before = d.disk();
    let mut lines: Vec<&str> = before.lines().collect();
    let done = lines.pop().expect("the last line is the done one");
    assert!(done.starts_with("x "), "{done}");
    lines.insert(0, done);
    let edited = lines.join("\n") + "\n";
    d.external_write(&edited);
    let after = d.settle().await;
    assert_eq!(after, edited, "the done line stays on top");
    assert_eq!(d.writes_since().await, 0, "the daemon wrote nothing back");
}
