//! txtodo-daemon's slow tests, one binary (task fast-gate): each starts at least one real
//! `txtodod` (and some use real mDNS, a real relay, or several daemons at once). 1-10 s per test.
//! Cargo.toml marks this target `test = false`, so `cargo test`, `cargo nextest run` and
//! `clippy --all-targets` leave it out; CI asks for it with `--test e2e`.
//! Run them here: `cargo nextest run -p txtodo-daemon --test e2e` (the default nextest profile
//! still skips the few that .config/nextest.toml lists; `--profile ci` runs all).
//! https://doc.rust-lang.org/cargo/reference/cargo-targets.html#the-test-field
#![cfg(unix)]

mod support;

mod crash;
mod default_workspace_foreign;
mod default_workspace_pairing;
mod default_workspace_sync;
mod dir_bridge_refused;
mod editor_saves;
mod external_edits;
mod external_edits_sidecar;
mod file_carrier_converge;
mod global_socket;
mod idle_rss;
mod lan_discovery;
mod lan_live_push;
mod lan_loopback_converge;
mod lan_sync_bench;
mod layout_sync;
mod layout_todo_file;
mod layout_toml;
mod logging_flow_sequence;
mod migrate_identity;
mod nested_ref_sync;
mod pairing_lan;
mod pairing_relay;
mod ready_log_ordering;
mod relay_auto_dial;
mod relay_converge;
mod relay_default;
mod relay_multiplex;
mod relay_node_id;
mod single_instance;
mod workspace_name;
mod workspace_rejoin;
