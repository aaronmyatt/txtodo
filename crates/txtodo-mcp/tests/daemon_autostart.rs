//! Real-`txtodod` proof for task `daemon-always-available`: no daemon running for a `--dir`-style
//! target, and `txtodo-mcp`'s startup path (`main.rs::ensure_daemon_for_target`, exercised here
//! directly via the same `LaunchConfig` shape it builds) spawns one and connects — without a
//! human ever running `txtodo daemon start`. Unlike `tests/global_workspace_routing.rs`'s own
//! real-daemon test, whose older harness's `daemon_binary()` only asserts `target/debug/txtodod`
//! already exists (a human must run `cargo build -p txtodo-daemon --bin txtodod` first),
//! `support::daemon_bin` below builds it on demand instead — the same approach
//! `crates/txtodo-tui/tests/support/mod.rs::daemon_bin` and `apps/desktop/src-tauri/tests/
//! support/mod.rs::TXTODOD_BIN` use for their own daemon-spawn tests, so it doesn't need a human
//! to pre-build anything.
//!
//! `#[ignore]`d anyway (2026-09-19): every sibling client's real-daemon test is now CI-only (see
//! `crates/txtodo-cli/tests/daemon_mode.rs`/`daemon_autostart.rs`, `apps/desktop/src-tauri/tests/
//! daemon_spawn.rs`) — a plain `cargo test -p txtodo-mcp` should stay consistent with the rest of
//! the workspace rather than being the one client whose real-daemon test still runs locally. Run
//! in CI via `cargo test -- --ignored` (`.github/workflows/ci.yml`).
//!
//! `run(args)` itself isn't easily unit-testable in isolation without also standing up the full
//! MCP stdio/HTTP transport, so this replicates the exact sequence `main.rs` performs instead:
//! build a hermetic `LaunchConfig` for the `Target::Dir` shape (a temp workspace, `daemon_bin`
//! pointed at the freshly built `txtodod`, no daemon pre-spawned), call `ensure_daemon`, then
//! assert `GrpcMcpBackend::connect_unix` succeeds afterward.

mod support;

use txtodo_mcp::backend::McpBackend;
use txtodo_mcp::grpc_backend::{GrpcMcpBackend, SOCKET_REL};

#[ignore = "spawns a real txtodod; CI-only, see ci.yml's --ignored step"]
#[tokio::test]
async fn ensure_daemon_spawns_and_connects_with_no_daemon_pre_started() {
    let workspace = support::temp_workspace();
    let socket = workspace.path().join(SOCKET_REL);
    assert!(
        !socket.exists(),
        "precondition: no daemon has bound this fresh workspace's socket yet"
    );

    // Same shape `main.rs::ensure_daemon_for_target` builds for `Target::Dir(dir)`: `.with_dir`
    // sets `extra_args` to `["--dir", "<workspace>"]`, and `daemon_bin` pins the freshly built
    // binary rather than searching `$PATH` (hermetic — no reliance on a dev machine's own install).
    let mut cfg = txtodo_daemon_launch::LaunchConfig::new(&socket).with_dir(workspace.path());
    cfg.daemon_bin = Some(support::daemon_bin().to_path_buf());

    txtodo_daemon_launch::ensure_daemon(&cfg)
        .await
        .unwrap_or_else(|e| panic!("ensure_daemon: {e}"));

    let backend = GrpcMcpBackend::connect_unix(&socket, None)
        .await
        .unwrap_or_else(|e| panic!("connect after ensure_daemon: {e}"));
    // Any cheap round-trip proves the connection is real and serving, not just that the unix
    // socket accepted a TCP-style handshake.
    backend
        .list_workspaces()
        .await
        .unwrap_or_else(|e| panic!("list_workspaces over the freshly spawned daemon: {e}"));

    // Clean up the daemon `ensure_daemon` spawned — it outlives this fn otherwise (`spawn.rs`
    // reaps the child on a background thread but never kills it on our behalf).
    let pid_path = workspace.path().join(".txtodo/txtodod.pid");
    let pid = support::wait_for_pid(&pid_path);
    support::kill(pid);
}
