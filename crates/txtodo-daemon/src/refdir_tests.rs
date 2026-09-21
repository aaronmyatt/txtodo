//! `refdir.rs`/`refdir_ops.rs`: slug generation, lazy creation as one op batch, collisions, and
//! rename with rollback on a forced failure.

use crate::actor::{ActorConfig, FileActor, SharedStore};
use crate::clock::FakeClock;
use crate::mutation::TaskRef;
use crate::refdir::generate_slug;
use crate::stats::Stats;
use std::path::Path;
use std::sync::{Arc, Mutex};
use txtodo_core::{LineKind, parse_line};
use txtodo_model::{DeviceId, FilePath, IdentityMode, Principal, Ulid};
use txtodo_store::Store;

fn device() -> DeviceId {
    DeviceId::new(Ulid::from_u128(9))
}

fn user() -> Principal {
    Principal::User { device: device() }
}

fn store(dir: &Path) -> SharedStore {
    Arc::new(Mutex::new(
        Store::open(&dir.join("oplog.db")).unwrap_or_else(|e| panic!("open store: {e}")),
    ))
}

fn cfg(dir: &Path) -> ActorConfig {
    ActorConfig {
        path: FilePath::new("todo.txt").unwrap_or_else(|e| panic!("{e}")),
        disk: dir.join("todo.txt"),
        device: device(),
        stats: Arc::new(Stats::default()),
        identity_mode: IdentityMode::Tagged,
        tree_dirty: Arc::new(crate::tree_dirty::TreeDirty::default()),
        layout: crate::layout_state::SharedLayout::default(),
    }
}

fn open(dir: &Path, store: &SharedStore) -> FileActor {
    let clock: Arc<dyn crate::clock::Clock> = Arc::new(FakeClock::new(1_000));
    FileActor::open(cfg(dir), Arc::clone(store), clock).unwrap_or_else(|e| panic!("{e}"))
}

fn disk(dir: &Path) -> String {
    String::from_utf8(std::fs::read(dir.join("todo.txt")).unwrap_or_default()).unwrap_or_default()
}

fn last_seq(store: &SharedStore) -> i64 {
    store
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .last_seq()
        .unwrap_or(None)
        .map_or(0, |s| s.0)
}

fn task(id: &str) -> Ulid {
    Ulid::parse(id).unwrap_or_else(|| panic!("bad ulid {id}"))
}

fn plain_words(description: &str) -> Vec<String> {
    let line = format!("x {description}");
    let LineKind::Task(t) = parse_line(&line, txtodo_core::Mode::Lenient)
        .unwrap_or_else(|e| panic!("{e}"))
        .kind
    else {
        panic!("not a task");
    };
    t.plain_words().map(str::to_owned).collect()
}

#[test]
fn generate_slug_kebab_cases_plain_words_and_truncates() {
    let id = crate::state::task_id(1);
    let words = plain_words("Learn C++ Today +work ref:x");
    let refs: Vec<&str> = words.iter().map(String::as_str).collect();
    assert_eq!(
        generate_slug(refs.into_iter(), id),
        "learn-c-today",
        "punctuation collapses to one dash, case folds"
    );
    let long = "a ".repeat(30);
    let words = plain_words(&long);
    let refs: Vec<&str> = words.iter().map(String::as_str).collect();
    let slug = generate_slug(refs.into_iter(), id);
    assert!(slug.chars().count() <= 40 && !slug.ends_with('-'), "{slug}");
}

#[test]
fn generate_slug_falls_back_to_the_task_id_with_no_ascii_plain_words() {
    let id = crate::state::task_id(0x01A2);
    let words = plain_words("买菜 +家务");
    assert!(words.iter().all(|w| w == "买菜"), "{words:?}");
    let refs: Vec<&str> = words.iter().map(String::as_str).collect();
    let slug = generate_slug(refs.into_iter(), id);
    assert!(
        txtodo_core::is_valid_slug(&slug),
        "the fallback is always a valid slug: {slug}"
    );
}

#[tokio::test]
async fn ensure_ref_dir_writes_the_tag_and_directory_in_one_op_batch() {
    let dir = tempfile::tempdir().unwrap();
    let store = store(dir.path());
    let handle = open(dir.path(), &store).spawn();
    handle
        .apply(
            vec![
                crate::mutation::Mutation::Add {
                    line: "(A) Q4 roadmap +work".into(),
                },
                crate::mutation::Mutation::Add {
                    line: "unrelated line".into(),
                },
            ],
            user(),
        )
        .await
        .unwrap();
    let before = disk(dir.path());
    let seq_before = last_seq(&store);
    let info = handle
        .ensure_ref_dir(
            TaskRef {
                line_number: 1,
                task_id: None,
            },
            user(),
        )
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(info.slug, "q4-roadmap");
    assert!(info.dir.is_dir(), "{info:?}");
    assert_eq!(last_seq(&store), seq_before + 1, "exactly one op landed");
    let after = disk(dir.path());
    let mut before_lines = before.lines();
    let mut after_lines = after.lines();
    assert_ne!(before_lines.next(), after_lines.next(), "line 1 changed");
    assert_eq!(
        before_lines.next(),
        after_lines.next(),
        "line 2 is untouched"
    );
    assert!(after.lines().next().unwrap().contains("ref:q4-roadmap"));
}

#[tokio::test]
async fn ensure_ref_dir_is_a_no_op_write_when_the_tag_already_exists() {
    let dir = tempfile::tempdir().unwrap();
    let store = store(dir.path());
    let handle = open(dir.path(), &store).spawn();
    handle
        .apply(
            vec![crate::mutation::Mutation::Add {
                line: "(A) roadmap ref:already".into(),
            }],
            user(),
        )
        .await
        .unwrap();
    let seq_before = last_seq(&store);
    let info = handle
        .ensure_ref_dir(
            TaskRef {
                line_number: 1,
                task_id: None,
            },
            user(),
        )
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(info.slug, "already");
    assert!(info.dir.is_dir());
    assert_eq!(last_seq(&store), seq_before, "a dangling ref costs no op");
}

#[tokio::test]
async fn a_slug_collision_appends_dash_2_then_dash_3() {
    let dir = tempfile::tempdir().unwrap();
    let store = store(dir.path());
    let handle = open(dir.path(), &store).spawn();
    handle
        .apply(
            vec![
                crate::mutation::Mutation::Add {
                    line: "(A) roadmap".into(),
                },
                crate::mutation::Mutation::Add {
                    line: "(A) roadmap".into(),
                },
                crate::mutation::Mutation::Add {
                    line: "(A) roadmap".into(),
                },
            ],
            user(),
        )
        .await
        .unwrap();
    let one = TaskRef {
        line_number: 1,
        task_id: None,
    };
    let two = TaskRef {
        line_number: 2,
        task_id: None,
    };
    let three = TaskRef {
        line_number: 3,
        task_id: None,
    };
    let a = handle.ensure_ref_dir(one, user()).await.unwrap();
    let b = handle.ensure_ref_dir(two, user()).await.unwrap();
    let c = handle.ensure_ref_dir(three, user()).await.unwrap();
    assert_eq!(
        (a.slug.as_str(), b.slug.as_str(), c.slug.as_str()),
        ("roadmap", "roadmap-2", "roadmap-3")
    );
    assert!(a.dir.is_dir() && b.dir.is_dir() && c.dir.is_dir());
}

#[tokio::test]
async fn a_dangling_ref_on_another_line_still_claims_its_slug() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("todo.txt"),
        format!(
            "(A) roadmap ref:roadmap id:{}\n(A) roadmap id:{}\n",
            task("01ARZ3NDEKTSV4RRFFQ69G5FAA"),
            task("01ARZ3NDEKTSV4RRFFQ69G5FAB")
        ),
    )
    .unwrap();
    let store = store(dir.path());
    let handle = open(dir.path(), &store).spawn();
    // Line 1's `ref:roadmap` is dangling (no directory on disk), but line 2 must still see the
    // slug as taken (rule 9) and skip to `-2`.
    let info = handle
        .ensure_ref_dir(
            TaskRef {
                line_number: 2,
                task_id: None,
            },
            user(),
        )
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(info.slug, "roadmap-2");
}

#[tokio::test]
async fn rename_ref_dir_moves_the_directory_and_rewrites_the_tag() {
    let dir = tempfile::tempdir().unwrap();
    let store = store(dir.path());
    let handle = open(dir.path(), &store).spawn();
    handle
        .apply(
            vec![crate::mutation::Mutation::Add {
                line: "(A) roadmap".into(),
            }],
            user(),
        )
        .await
        .unwrap();
    let line = TaskRef {
        line_number: 1,
        task_id: None,
    };
    handle.ensure_ref_dir(line.clone(), user()).await.unwrap();
    let renamed = handle
        .rename_ref_dir(line, "q4-plan".into(), user())
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(renamed.slug, "q4-plan");
    assert!(renamed.dir.is_dir());
    assert!(!dir.path().join("roadmap").exists());
    assert!(disk(dir.path()).contains("ref:q4-plan"));
}

#[tokio::test]
async fn rename_ref_dir_rejects_a_slug_already_claimed_in_this_document() {
    let dir = tempfile::tempdir().unwrap();
    let store = store(dir.path());
    let handle = open(dir.path(), &store).spawn();
    handle
        .apply(
            vec![
                crate::mutation::Mutation::Add {
                    line: "(A) roadmap ref:taken".into(),
                },
                crate::mutation::Mutation::Add {
                    line: "(A) other".into(),
                },
            ],
            user(),
        )
        .await
        .unwrap();
    let other = TaskRef {
        line_number: 2,
        task_id: None,
    };
    handle.ensure_ref_dir(other.clone(), user()).await.unwrap();
    let err = handle
        .rename_ref_dir(other, "taken".into(), user())
        .await
        .unwrap_err();
    assert!(err.to_string().contains("taken"), "{err}");
}

#[tokio::test]
async fn a_failed_directory_rename_rolls_back_the_tag() {
    let dir = tempfile::tempdir().unwrap();
    let store = store(dir.path());
    let handle = open(dir.path(), &store).spawn();
    handle
        .apply(
            vec![crate::mutation::Mutation::Add {
                line: "(A) roadmap".into(),
            }],
            user(),
        )
        .await
        .unwrap();
    let line = TaskRef {
        line_number: 1,
        task_id: None,
    };
    handle.ensure_ref_dir(line.clone(), user()).await.unwrap();
    // Block the rename target with a plain file: renaming a directory onto it must fail.
    std::fs::write(dir.path().join("blocked"), b"not a directory").unwrap();
    let before = disk(dir.path());
    let err = handle
        .rename_ref_dir(line, "blocked".into(), user())
        .await
        .unwrap_err();
    assert!(matches!(err, crate::handle::ActorError::RefDir(_)), "{err}");
    assert_eq!(
        disk(dir.path()),
        before,
        "the tag rollback restores the line"
    );
    assert!(
        dir.path().join("roadmap").is_dir(),
        "the old directory is untouched"
    );
}
