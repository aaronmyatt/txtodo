//! txtodo-daemon-launch's slow tests, one binary (task fast-gate): each one starts a real
//! `txtodod`, and `support` first builds it with a nested `cargo build`. ~67 s per test locally.
//! Cargo.toml marks this target `test = false`, so `cargo test`, `cargo nextest run` and
//! `clippy --all-targets` leave it out; CI asks for it with `--test e2e`.
//! The fast, in-process tests (`tests/autostart.rs`, `tests/service_disabled.rs`) stay separate
//! binaries on purpose: each mutates a process-global env var, and plain `cargo test` runs a
//! binary's tests as threads of one process.
//! Run them here: `cargo nextest run -p txtodo-daemon-launch --test e2e`.
//! https://doc.rust-lang.org/cargo/reference/cargo-targets.html#the-test-field
#![cfg(unix)]

mod support;

mod ensure_daemon;
mod simulated_reboot;
mod upgrade;
