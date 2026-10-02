//! txtodo-mcp's fast integration tests, one binary (task fast-gate). None starts a `txtodod`:
//! `smoke` runs `McpServer` in-process against a fake backend, the http tests bind the transport
//! alone, `version` runs the `txtodo-mcp` binary. The real-daemon tests are in `tests/e2e`.
//! Pattern: https://matklad.github.io/2021/02/27/delete-cargo-integration-tests.html

/// The in-memory backend `smoke` and the http tests share.
mod fake_backend;
mod http_guard;
mod http_loopback_bind;
mod smoke;
mod version;
