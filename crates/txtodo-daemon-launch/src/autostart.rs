//! The `TXTODO_NO_AUTOSTART` escape hatch (task `daemon-always-available`'s open question,
//! resolved: yes, every client needs one). `apps/desktop` honors it too as of task
//! `desktop-autostart-env-respect` (`commands.rs::connect_and_store`) — it had been the one
//! silent exception, always-spawning unconditionally regardless of the var.

/// True when `$TXTODO_NO_AUTOSTART` is set to exactly `"1"`. Callers check this before calling
/// [`crate::ensure_daemon`] at all — never inside it — so a caller that wants to skip the
/// persistent-service step too (not just the ad-hoc spawn) has a single, obvious place to do it.
///
/// Tested from `tests/autostart.rs`, not a `#[cfg(test)]` module here: exercising this needs
/// `std::env::set_var`, `unsafe` under Rust 2024, and this crate's root has `#![forbid
/// (unsafe_code)]` — a separate integration-test binary is its own compilation unit and isn't
/// covered by that, the same pattern `crates/txtodo-daemon/tests/debug_hooks.rs` already uses.
pub fn autostart_disabled() -> bool {
    std::env::var_os("TXTODO_NO_AUTOSTART").is_some_and(|v| v == "1")
}
