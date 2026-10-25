//! A real `txtodod` in true global mode (`--dir` omitted), hermetic via `$TXTODO_SOCKET`/
//! `$TXTODO_REGISTRY_DB` so it never touches this machine's real `$XDG_DATA_HOME/txtodo/`.
//! The same harness `workspace.rs` and `layout.rs` each carry inline; new tests use this copy.
#![allow(dead_code)] // each integration test binary compiles `support`, not all use this file

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

const SOCKET_WAIT: Duration = Duration::from_secs(20);

/// The spawned daemon; killed on drop.
pub struct GlobalDaemon {
    child: Child,
    /// The unix socket the daemon listens on.
    pub socket: PathBuf,
}

impl GlobalDaemon {
    /// Spawns the daemon with its socket and registry under `state_dir`, waiting for the socket.
    pub fn spawn(state_dir: &Path) -> GlobalDaemon {
        Self::spawn_with(state_dir, &[])
    }

    /// `spawn`, with more environment variables on the daemon (e.g. a faster redial for a test that
    /// waits on sync between two daemons).
    pub fn spawn_with(state_dir: &Path, envs: &[(&str, &str)]) -> GlobalDaemon {
        let socket = state_dir.join("txtodod.sock");
        let child = Command::new(super::txtodod_binary())
            .envs(envs.iter().copied())
            .env("TXTODO_SOCKET", &socket)
            .env("TXTODO_REGISTRY_DB", state_dir.join("registry.db"))
            // Debug-only seam (`identity_setup.rs`): a freshly rebuilt `txtodod` otherwise blocks
            // in the OS keychain prompt on a developer Mac, past `SOCKET_WAIT`.
            .env("TXTODO_TEST_KEYSTORE_MEMORY", "1")
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap_or_else(|e| panic!("spawn txtodod: {e}"));
        let start = Instant::now();
        while !socket.exists() {
            assert!(
                start.elapsed() < SOCKET_WAIT,
                "daemon socket did not appear"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
        GlobalDaemon { child, socket }
    }

    /// Runs `txtodo` against this daemon's socket, from `dir` — which must have no
    /// `.txtodo/txtodod.sock` of its own, so `client::select` only reaches the global daemon.
    pub fn txtodo(&self, dir: &Path, args: &[&str]) -> Output {
        self.command(dir)
            .args(args)
            .output()
            .unwrap_or_else(|e| panic!("txtodo: {e}"))
    }

    /// The `txtodo` command `txtodo` runs, for a test that needs to feed it stdin or keep it running.
    pub fn command(&self, dir: &Path) -> Command {
        assert!(
            !dir.join(".txtodo").join("txtodod.sock").exists(),
            "this harness only proves the global-daemon path"
        );
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_txtodo"));
        cmd.current_dir(dir)
            .env_remove("TXTODO_TODO_DIR")
            .env("TXTODO_CONFIG", dir.join("none.toml"))
            .env("TXTODO_SOCKET", &self.socket)
            // A test daemon that is not up must fail this test, never make the CLI spawn a
            // daemon of its own at this socket with the machine's real registry.
            .env("TXTODO_NO_AUTOSTART", "1");
        cmd
    }
}

impl Drop for GlobalDaemon {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
