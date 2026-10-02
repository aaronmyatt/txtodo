//! desktop's real-daemon tests, one binary (task fast-gate): each starts a real `txtodod`, which
//! `support::TXTODOD_BIN` builds first with a nested `cargo build`. Cargo.toml marks this target
//! `test = false`, so `cargo test`, `cargo nextest run` and `clippy --all-targets` leave it out;
//! CI asks for it with `--test e2e`. Until this move each was `#[ignore]`d (run by CI's
//! `--run-ignored` step) for the same reason. This crate has no fast integration tests, so no
//! `tests/it`. Run them here: `cargo nextest run -p desktop --test e2e`.
//! https://doc.rust-lang.org/cargo/reference/cargo-targets.html#the-test-field
#![cfg(unix)]

mod support;

mod daemon_spawn;
mod new_rpcs;
mod universal_view;
mod workspace_registry;
