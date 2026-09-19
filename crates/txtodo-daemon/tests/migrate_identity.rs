//! `MigrateIdentity` against a real `txtodod` (tasks/sidecar-migrate-tagged, ADR 0019): a Tagged
//! workspace loses every `id:` tag, each task keeps the history it already had, and afterwards the
//! workspace behaves as Sidecar — an external edit is re-identified by fingerprint and nothing is
//! ever stamped back into the file.
// Integration tests are tests: clippy.toml allows unwrap/expect in #[test] fns but not in their helpers.
#![allow(clippy::expect_used, clippy::unwrap_used)]
// The daemon listens on a unix-domain socket (ADR 0010); these scenarios cannot run on Windows.
#![cfg(unix)]

mod support;

use support::{Daemon, kinds};

const TODO: &str = "(A) 2026-09-11 buy ducks +farm\n\nwalk the dog @home\nx 2026-09-11 2026-09-10 call mum @phone\n";

fn tag_count(text: &str) -> usize {
    text.split_whitespace()
        .filter(|w| w.starts_with("id:"))
        .count()
}

#[tokio::test]
async fn a_tagged_workspace_migrates_keeping_history_and_stays_untagged() {
    let mut d = Daemon::start_with_mode(TODO, "tagged").await;
    assert_eq!(tag_count(&d.disk()), 3, "tagged mode stamps every task");

    // History before the migration: one real edit to the first task.
    let tagged = d.disk().replacen("buy ducks", "buy 400 ducks", 1);
    d.external_write(&tagged);
    d.settle().await;
    let edit = d.history().await;
    let duck_task = edit
        .iter()
        .find(|o| o.kind == "edit_text")
        .expect("the edit is recorded")
        .task_id
        .clone();
    assert!(!duck_task.is_empty());

    // A dry run counts and changes nothing.
    let before = d.disk();
    let plan = d.migrate_identity(true).await;
    assert!(plan.was_tagged);
    assert_eq!((plan.files, plan.tasks, plan.stripped), (1, 3, 3));
    assert!(plan.failures.is_empty());
    assert_eq!(d.disk(), before);

    // The real thing.
    let done = d.migrate_identity(false).await;
    assert!(done.was_tagged);
    assert_eq!(done.stripped, 3);
    assert!(done.failures.is_empty(), "{:?}", done.failures);
    let after = d.settle().await;
    assert_eq!(tag_count(&after), 0, "no id: left: {after}");
    assert_eq!(
        after,
        "(A) 2026-09-11 buy 400 ducks +farm\n\nwalk the dog @home\nx 2026-09-11 2026-09-10 call mum @phone\n"
    );

    // The task's history survived, and the migration is one more ordinary edit on it.
    let history = d.task_history(&duck_task).await;
    assert_eq!(kinds(&history), vec!["insert", "edit_text", "edit_text"]);

    // Idempotent: a second run has nothing left to do and touches nothing.
    let again = d.migrate_identity(false).await;
    assert!(!again.was_tagged);
    assert_eq!(again.stripped, 0);
    assert_eq!(d.disk(), after);

    // Sidecar from here on: an external edit re-identifies by fingerprint (the same task id gains
    // another edit) and nothing is stamped back.
    let edited = after.replacen("buy 400 ducks", "buy 500 ducks", 1);
    d.external_write(&edited);
    let settled = d.settle().await;
    assert_eq!(settled, edited);
    assert_eq!(tag_count(&settled), 0);
    let history = d.task_history(&duck_task).await;
    assert_eq!(
        kinds(&history),
        vec!["insert", "edit_text", "edit_text", "edit_text"]
    );

    // A brand-new line stays untagged too.
    d.external_write(&format!("{settled}call the bank\n"));
    let with_new = d.settle().await;
    assert!(with_new.ends_with("call the bank\n"), "{with_new}");
    assert_eq!(tag_count(&with_new), 0);
}
