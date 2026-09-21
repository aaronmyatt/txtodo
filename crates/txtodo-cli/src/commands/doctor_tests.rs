//! `doctor.rs`'s tests, split out for the 400-line file cap (same idiom as
//! `txtodo-daemon`'s `*_tests.rs` siblings).

use super::*;

fn healthy() -> pb::HealthResponse {
    pb::HealthResponse {
        watcher_alive: true,
        documents: 1,
        version: "0.0.0".into(),
        key_store_backend: "os".into(),
        ..pb::HealthResponse::default()
    }
}

#[test]
fn no_daemon_is_unknown_not_a_failure() {
    let c = keystore_check(None);
    assert_eq!(c.status, Status::Warn);
    assert!(c.detail.contains("no daemon"));
}

#[test]
fn a_real_backend_is_ok() {
    let h = healthy();
    let c = keystore_check(Some(&h));
    assert_eq!(c.status, Status::Ok);
    assert!(c.detail.contains("os"));
}

/// task `relay-id-keystore`: a `"memory"` backend is a FAIL, not a warn — the daemon found no
/// keychain and no `--key-store file` was given, so the relay identity does not survive a restart.
#[test]
fn a_memory_backend_fails() {
    let h = pb::HealthResponse {
        key_store_backend: "memory".into(),
        ..healthy()
    };
    let c = keystore_check(Some(&h));
    assert_eq!(c.status, Status::Fail);
    assert!(c.detail.contains("memory"));
}

#[test]
fn an_empty_backend_is_a_warn_not_a_failure() {
    let h = pb::HealthResponse {
        key_store_backend: String::new(),
        ..healthy()
    };
    let c = keystore_check(Some(&h));
    assert_eq!(c.status, Status::Warn);
}
