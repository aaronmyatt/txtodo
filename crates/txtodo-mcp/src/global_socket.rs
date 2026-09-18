//! Where the device-global `txtodod` listens, for `main.rs`'s `--global` mode (mcp-multi-
//! workspace-gateway): reachable at one socket regardless of which workspace(s) it has open,
//! unlike the pre-existing `--dir` bridge (`grpc_backend.rs::SOCKET_REL`), which only ever reaches
//! whatever single directory's own daemon a caller started. Delegates to `txtodo-workspace-paths`
//! (task `daemon-paths-shared-crate`) — this crate previously reimplemented the same resolution
//! by hand, since it may not depend on `txtodo-daemon` directly (the dependency runs the other
//! way: the daemon depends on this crate, `budgets.json`'s `allowedDeps`); the shared crate has no
//! such constraint, being a dependency-free leaf every client can take.

use std::path::PathBuf;
use txtodo_workspace_paths::RegistryEnv;

/// `$TXTODO_SOCKET` if set; else the platform data directory (`$XDG_DATA_HOME`/`%LOCALAPPDATA%`/
/// `~/.local/share`, falling back to the cwd when none resolve) + `txtodo/txtodod.sock`.
pub fn path() -> PathBuf {
    let env = RegistryEnv::from_process().unwrap_or_default();
    txtodo_workspace_paths::global_socket_path(&env, None)
}

/// The log directory sitting beside [`path`]'s socket.
pub fn log_dir() -> PathBuf {
    let env = RegistryEnv::from_process().unwrap_or_default();
    txtodo_workspace_paths::global_log_dir(&env)
}
