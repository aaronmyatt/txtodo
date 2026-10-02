//! txtodo-tui's slow tests, one binary (task fast-gate): each drives the app against a real
//! `txtodod` through `support`. Cargo.toml marks this target `test = false`, so `cargo test`,
//! `cargo nextest run` and `clippy --all-targets` leave it out; CI asks for it with `--test e2e`.
//! Until this move most were `#[ignore]`d (run by CI's `--run-ignored` step) for the same reason.
//! Run them here: `cargo nextest run -p txtodo-tui --test e2e`.
//! https://doc.rust-lang.org/cargo/reference/cargo-targets.html#the-test-field
#![cfg(unix)]

mod support;

mod daemon_autostart;
mod daemon_wrappers;
mod detail_panel;
mod external_edit;
mod list_mode;
mod mouse_drag;
mod prompt_bar;
mod ref_badges;
mod refused_edit;
mod roundtrip;
mod sentinel_no_secrets;
mod settings_screen;
mod sync_status;
mod toast_undo;
mod universal_screen;
mod workspace_switch;
