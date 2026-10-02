//! txtodo-mcp's slow tests, one binary (task fast-gate): each starts a real `txtodod`, which
//! `support::daemon_bin` builds with a nested `cargo build` when `target/debug/txtodod` is missing
//! or empty. Cargo.toml marks this target `test = false`, so `cargo test`, `cargo nextest run`
//! and `clippy --all-targets` leave it out; CI asks for it with `--test e2e`. Until this move each
//! was `#[ignore]`d (run by CI's `--run-ignored` step) for the same reason.
//! Run them here: `cargo nextest run -p txtodo-mcp --test e2e`.
//! https://doc.rust-lang.org/cargo/reference/cargo-targets.html#the-test-field
#![cfg(unix)]

mod support;

mod cwd_autoregister;
mod daemon_autostart;
mod dir_owned_by_global;
mod global_workspace_routing;
mod sdk_client;
mod sidecar_id_tools;
