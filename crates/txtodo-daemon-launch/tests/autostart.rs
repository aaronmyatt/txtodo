//! [`autostart_disabled`] against a real env var mutation. A separate integration-test binary
//! (not a `#[cfg(test)]` module in `src/autostart.rs`): mutating the process environment needs
//! `unsafe` under Rust 2024, and the library crate's root has `#![forbid(unsafe_code)]`, which
//! does not reach a separate compilation unit under `tests/` — the same pattern
//! `crates/txtodo-daemon/tests/debug_hooks.rs` already uses for the same reason.
//!
//! One `#[test]`, not several: `TXTODO_NO_AUTOSTART` is process-global state, and `cargo test`
//! runs every test in a binary on one process by default, so two tests each mutating it would
//! race each other.

use txtodo_daemon_launch::autostart_disabled;

#[test]
fn reads_the_env_var_exactly() {
    // SAFETY: this test binary touches `TXTODO_NO_AUTOSTART` only here, in one single-threaded
    // `#[test]` fn, so there is no concurrent access to race.
    unsafe {
        std::env::remove_var("TXTODO_NO_AUTOSTART");
    }
    assert!(!autostart_disabled(), "unset means autostart stays on");

    // SAFETY: see above.
    unsafe {
        std::env::set_var("TXTODO_NO_AUTOSTART", "1");
    }
    assert!(autostart_disabled());

    // SAFETY: see above.
    unsafe {
        std::env::set_var("TXTODO_NO_AUTOSTART", "true");
    }
    assert!(
        !autostart_disabled(),
        "only the exact value \"1\" opts out, an unambiguous flag rather than a fuzzy truthy \
         string"
    );

    // SAFETY: see above.
    unsafe {
        std::env::remove_var("TXTODO_NO_AUTOSTART");
    }
}
