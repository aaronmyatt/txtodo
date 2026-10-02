//! txtodo-cli's slow tests, one binary (task fast-gate): each starts a real `txtodod` (built by
//! `support` with a nested `cargo build` when missing), and `pairing` uses real mDNS.
//! Cargo.toml marks this target `test = false`, so `cargo test`, `cargo nextest run` and
//! `clippy --all-targets` leave it out; CI asks for it with `--test e2e`. Most of these were
//! `#[ignore]`d (and run by CI's `--run-ignored` step) for the same reason until this move.
//! Run them here: `cargo nextest run -p txtodo-cli --test e2e`.
//! https://doc.rust-lang.org/cargo/reference/cargo-targets.html#the-test-field
#![cfg(unix)]

mod support;

mod bundle;
mod conflicts_dup;
mod daemon_archive;
mod daemon_autostart;
mod daemon_mode;
mod layout;
mod nested_ref_sync;
mod pairing;
mod sub_scope;
mod workspace;
mod workspace_offers;
