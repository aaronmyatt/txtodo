//! txtodo-daemon's in-process integration tests, one binary (task fast-gate). Each file here was
//! its own `tests/*.rs` binary, and each one relinked the whole daemon on every edit. As modules
//! of one crate they link once. None of these starts a real `txtodod`: they serve a `Workspace`
//! in-process (or scan the source tree). The real-daemon tests are in `tests/e2e`.
//! `tests/debug_hooks.rs` stays its own binary: it sets a process-global env var.
//! Pattern: https://matklad.github.io/2021/02/27/delete-cargo-integration-tests.html

mod activity;
mod apply_dry_run;
mod apply_new_list;
mod concurrent_apply;
mod get_file_task_ids;
mod grpc;
mod grpc_conflicts;
mod layout_rpc;
mod m5_acceptance;
mod mcp_backend;
mod mcp_batch_dry_run;
mod mutation_placement;
mod notes_grpc;
mod op_source;
mod reopen_apply;
mod replace_apply;
mod security_keys_only_in_keystore;
mod tokens;
