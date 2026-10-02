//! txtodo-cli's fast integration tests, one binary (task fast-gate). Each file here was its own
//! `tests/*.rs` binary, and each one relinked the CLI on every edit. As modules of one crate they
//! link once. None of these starts a `txtodod`: they run the `txtodo` binary in direct-file mode
//! (and todo.sh beside it). The real-daemon tests are in `tests/e2e`.
//! Pattern: https://matklad.github.io/2021/02/27/delete-cargo-integration-tests.html

mod add;
mod archive;
mod env;
mod hygiene;
mod list;
mod sentinel_no_secrets;
mod socket_paths;
mod todosh_parity;
