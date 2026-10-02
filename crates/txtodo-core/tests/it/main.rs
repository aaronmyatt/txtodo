//! txtodo-core's integration tests, one binary: corpus, edge cases, proptest properties, differential.
//! Each file here was its own `tests/*.rs` binary, and each one relinked the crate on every
//! edit; as modules of one binary they link once (task fast-gate). Every test is in-process, so
//! this crate has no `tests/e2e`.
//! Pattern: https://matklad.github.io/2021/02/27/delete-cargo-integration-tests.html

mod corpus;
mod differential;
mod edge_cases;
mod props;
