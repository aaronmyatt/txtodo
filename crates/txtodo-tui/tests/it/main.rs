//! txtodo-tui's fast integration tests, one binary (task fast-gate). Neither starts a `txtodod`.
//! The real-daemon tests are in `tests/e2e`.
//! Pattern: https://matklad.github.io/2021/02/27/delete-cargo-integration-tests.html

mod complete_keeps_row;
mod parity;
