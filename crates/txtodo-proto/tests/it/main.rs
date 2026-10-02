//! txtodo-proto's integration tests, one binary: wire round trips.
//! Each file here was its own `tests/*.rs` binary, and each one relinked the crate on every
//! edit; as modules of one binary they link once (task fast-gate). Every test is in-process, so
//! this crate has no `tests/e2e`.
//! Pattern: https://matklad.github.io/2021/02/27/delete-cargo-integration-tests.html

mod roundtrip;
mod roundtrip_duplicates;
mod roundtrip_rejoin;
mod roundtrip_rename;
