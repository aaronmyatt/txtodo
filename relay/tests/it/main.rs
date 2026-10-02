//! relay's integration tests, one binary: docs checks, the HTTP smoke and the log sentinel.
//! Each file here was its own `tests/*.rs` binary, and each one relinked the crate on every
//! edit; as modules of one binary they link once (task fast-gate). Every test is in-process, so
//! this crate has no `tests/e2e`.
//! Pattern: https://matklad.github.io/2021/02/27/delete-cargo-integration-tests.html

mod docs_mention_optional;
mod help_matches_docs;
mod http_smoke;
mod no_secrets_sentinel;
mod no_txtodo_deps;
