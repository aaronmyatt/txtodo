//! txtodo-store's integration tests, one binary (task fast-gate). Each file here was its own
//! `tests/*.rs` binary, and each binary relinked the whole crate on every edit: 13 links for one
//! changed line. As modules of one crate they link once. Every test is in-process and fast, so
//! this crate has no `tests/e2e`.
//! Pattern: https://matklad.github.io/2021/02/27/delete-cargo-integration-tests.html

mod commit;
mod devices;
mod flags;
mod heads;
mod identity;
mod identity_store;
mod no_secrets_sentinel;
mod op_source;
mod oplog;
mod projections;
mod registry;
mod tokens;
