//! The `TXTODO_NO_SERVICE=1` guard: what it reads, that it stops `launchctl`/`systemctl`, and that
//! `.cargo/config.toml`'s `[env]` really puts it in every test process — the whole point, since
//! launchd's job label is one fixed name per *user*, so a test with temp `$HOME`/socket/registry
//! still `bootstrap`s/`bootout`s the developer's real `com.txtodo.txtodod` job without it.
//!
//! One `#[test]`, in its own binary: it mutates a process-global env var (`unsafe` under Rust
//! 2024, and only a separate `tests/` compilation unit escapes the lib's `#![forbid(unsafe_code)]`
//! — same pattern as `tests/autostart.rs`), so a second test here would race it.
//! `std::env::set_var`: <https://doc.rust-lang.org/std/env/fn.set_var.html>
#![cfg(unix)]

use std::path::PathBuf;
use txtodo_daemon_launch::service::{self, Rendered, ServiceError};
use txtodo_daemon_launch::service_disabled;

#[test]
fn guard_reads_exactly_one_and_blocks_the_service_manager() {
    // Read before any mutation. Fails loudly (rather than silently exposing the real launchd job)
    // when a runner ignores `.cargo/config.toml`'s `[env]` or the caller overrode it.
    assert_eq!(
        std::env::var("TXTODO_NO_SERVICE").as_deref(),
        Ok("1"),
        "`cargo test` must run with TXTODO_NO_SERVICE=1 (set by .cargo/config.toml's [env]); \
         without it a test can bootout the real `com.txtodo.txtodod` launchd job"
    );
    assert!(service_disabled());

    // SAFETY: this is the only test in this binary, so nothing else reads or writes the
    // environment concurrently.
    unsafe {
        std::env::set_var("TXTODO_NO_SERVICE", "true");
    }
    assert!(
        !service_disabled(),
        "only the exact value \"1\" disables, an unambiguous flag rather than a fuzzy truthy string"
    );
    // SAFETY: see above.
    unsafe {
        std::env::remove_var("TXTODO_NO_SERVICE");
    }
    assert!(
        !service_disabled(),
        "unset means the service manager stays reachable"
    );

    // SAFETY: see above.
    unsafe {
        std::env::set_var("TXTODO_NO_SERVICE", "1");
    }
    // A label no real unit uses: if the guard ever regressed this would only ask launchd/systemd
    // to stop something that was never loaded, never the developer's real job.
    let never_loaded = Rendered {
        label: "com.txtodo.test-never-loaded".to_owned(),
        path: PathBuf::from("/nonexistent/com.txtodo.test-never-loaded.plist"),
        body: String::new(),
    };
    let Err(err) = service::stop(&never_loaded) else {
        panic!("service::stop must refuse while TXTODO_NO_SERVICE=1");
    };
    assert!(
        matches!(&err, ServiceError::Message(m) if m.contains("TXTODO_NO_SERVICE")),
        "expected the guard's own refusal, not a launchctl/systemctl failure: {err:?}"
    );
}
