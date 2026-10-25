//! The registry's repo-scan state: roots added, removed and seeded once, the last scan time, and
//! that an older registry gains the tables without a schema bump.
// Integration tests are tests: clippy.toml allows unwrap/expect in #[test] fns but not in their helpers.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::path::Path;
use txtodo_store::{MAX_SCAN_ROOTS, Registry, StoreError};

fn open(dir: &Path) -> Registry {
    Registry::open(&dir.join("registry.db")).unwrap_or_else(|e| panic!("open: {e}"))
}

fn roots(r: &[&str]) -> Vec<String> {
    r.iter().map(|s| (*s).to_owned()).collect()
}

#[test]
fn a_fresh_registry_seeds_its_defaults_once_even_after_they_are_removed() {
    let dir = tempfile::tempdir().unwrap();
    let mut registry = open(dir.path());
    assert!(registry.scan_roots().unwrap().is_empty());

    assert!(
        registry
            .seed_scan_roots(&roots(&["/home/a"]), 1_000)
            .unwrap()
    );
    assert_eq!(registry.scan_roots().unwrap(), ["/home/a"]);
    assert!(
        !registry
            .seed_scan_roots(&roots(&["/home/b"]), 2_000)
            .unwrap(),
        "once"
    );

    assert!(registry.remove_scan_root("/home/a").unwrap());
    assert!(registry.scan_roots().unwrap().is_empty());
    assert!(
        !registry
            .seed_scan_roots(&roots(&["/home/a"]), 3_000)
            .unwrap(),
        "a root the person removed does not come back"
    );
    assert!(registry.scan_roots().unwrap().is_empty());
}

#[test]
fn roots_are_added_once_listed_oldest_first_and_survive_a_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let mut registry = open(dir.path());
    assert!(registry.add_scan_root("/work", 2_000).unwrap());
    assert!(registry.add_scan_root("/home/a", 1_000).unwrap());
    assert!(
        !registry.add_scan_root("/work", 3_000).unwrap(),
        "already there"
    );
    assert_eq!(registry.scan_roots().unwrap(), ["/home/a", "/work"]);
    assert!(!registry.remove_scan_root("/nowhere").unwrap());
    drop(registry);
    assert_eq!(open(dir.path()).scan_roots().unwrap(), ["/home/a", "/work"]);
}

#[test]
fn a_root_must_be_an_absolute_path_and_there_is_a_cap() {
    let dir = tempfile::tempdir().unwrap();
    let mut registry = open(dir.path());
    for bad in ["", "relative/dir", &format!("/{}", "x".repeat(5_000))] {
        assert!(matches!(
            registry.add_scan_root(bad, 1),
            Err(StoreError::BadScanRoot(_))
        ));
    }
    assert!(
        registry.add_scan_root("C:\\Users\\a", 1).is_ok(),
        "a drive path is absolute"
    );
    for i in 1..MAX_SCAN_ROOTS {
        registry.add_scan_root(&format!("/r{i}"), 1).unwrap();
    }
    assert!(matches!(
        registry.add_scan_root("/one-too-many", 1),
        Err(StoreError::TooManyScanRoots)
    ));
}

#[test]
fn the_last_scan_time_is_none_until_one_is_recorded_and_then_the_latest() {
    let dir = tempfile::tempdir().unwrap();
    let mut registry = open(dir.path());
    assert_eq!(registry.last_scan_ms().unwrap(), None);
    registry.set_last_scan_ms(5_000).unwrap();
    registry.set_last_scan_ms(9_000).unwrap();
    drop(registry);
    assert_eq!(open(dir.path()).last_scan_ms().unwrap(), Some(9_000));
}

/// The tables are not a migration: an older build, which reads neither, keeps opening the file.
#[test]
fn a_registry_from_before_scan_roots_opens_and_stays_at_schema_2() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("registry.db");
    let old = rusqlite::Connection::open(&path).unwrap();
    old.execute_batch(include_str!("../../registry_migrations/0001.sql"))
        .unwrap();
    old.execute_batch(include_str!("../../registry_migrations/0002.sql"))
        .unwrap();
    drop(old);

    let mut registry = Registry::open(&path).unwrap();
    assert!(registry.add_scan_root("/home/a", 1).unwrap());
    drop(registry);

    let conn = rusqlite::Connection::open(&path).unwrap();
    let version: i64 = conn
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap();
    assert_eq!(
        version, 2,
        "no schema bump, so an older build still opens it"
    );
}
