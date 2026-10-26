//! Task mcp-cwd-autoregister: `txtodo-mcp` started with no flags in a stray folder that happens
//! to hold a `todo.txt` must not register that folder (its name would reach every paired peer
//! while `outbound_offers` announced every registry entry). It serves the default workspace
//! instead and says so. Since task two-kinds-of-workspace only lists are offered, so a git
//! checkout's root with a list is served as itself: nothing about it can reach a peer. Real
//! `txtodod` + the real `txtodo-mcp` binary, in `tests/e2e`, like `global_workspace_routing.rs`.
#![allow(clippy::expect_used)]

use crate::support;

use std::path::{Path, PathBuf};
use std::time::Duration;

use txtodo_mcp::backend::McpBackend;
use txtodo_mcp::grpc_backend::GrpcMcpBackend;

/// `target/debug/txtodod`, built on first use by [`support::daemon_bin`] (this crate may not
/// depend on txtodo-daemon, so `CARGO_BIN_EXE_txtodod` does not exist here).
fn daemon_binary() -> PathBuf {
    support::daemon_bin().to_path_buf()
}

struct KillOnDrop(std::process::Child);
impl Drop for KillOnDrop {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

async fn wait_for_socket(path: &Path) {
    for _ in 0..100 {
        if path.exists() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("socket {path:?} never appeared");
}

/// A daemon on `tmp`'s socket and registry, up; and the two paths.
async fn start_daemon(tmp: &Path) -> (KillOnDrop, PathBuf, PathBuf) {
    let socket = tmp.join("txtodod.sock");
    let registry_db = tmp.join("registry.db");
    let daemon = KillOnDrop(
        std::process::Command::new(daemon_binary())
            .args(["--no-lan", "--no-relay"])
            .env("TXTODO_SOCKET", &socket)
            .env("TXTODO_REGISTRY_DB", &registry_db)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("spawn txtodod"),
    );
    wait_for_socket(&socket).await;
    (daemon, socket, registry_db)
}

/// What the server with neither --dir nor --global says on stderr when started in `dir`: it aims
/// at the folder when it can, and otherwise says it is not serving it and moves on.
async fn server_stderr_in(dir: &Path, socket: &Path, registry_db: &Path) -> String {
    let mut server = std::process::Command::new(env!("CARGO_BIN_EXE_txtodo-mcp"))
        .arg("--stdio")
        .current_dir(dir)
        .env("TXTODO_SOCKET", socket)
        .env("TXTODO_REGISTRY_DB", registry_db)
        .env("TXTODO_NO_AUTOSTART", "1")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("spawn txtodo-mcp");
    tokio::time::sleep(Duration::from_secs(2)).await;
    let _ = server.kill();
    let output = server.wait_with_output().expect("server output");
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[tokio::test]
async fn a_git_root_with_a_list_is_served_as_itself_not_the_default() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let (_daemon, socket, registry_db) = start_daemon(tmp.path()).await;
    let repo = tmp.path().join("a-repo");
    std::fs::create_dir_all(repo.join(".git")).expect("mkdir repo");
    std::fs::write(repo.join("todo.txt"), "a repo task\n").expect("seed todo.txt");
    let stderr = server_stderr_in(&repo, &socket, &registry_db).await;
    assert!(
        !stderr.contains("does not know") && !stderr.contains("no workspace here"),
        "a repo is its own workspace, with no fallback note: {stderr}"
    );
}

#[tokio::test]
async fn a_stray_todo_txt_folder_is_served_as_the_default_and_never_registered() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let (_daemon, socket, registry_db) = start_daemon(tmp.path()).await;
    let stray = tmp.path().join("client-acme-nda");
    std::fs::create_dir_all(&stray).expect("mkdir stray");
    std::fs::write(stray.join("todo.txt"), "secret client task\n").expect("seed todo.txt");

    // Started inside the stray folder, which the daemon does not know and no git root holds.
    let stderr = server_stderr_in(&stray, &socket, &registry_db).await;
    assert!(
        stderr.contains("does not know") && stderr.contains("client-acme-nda"),
        "the server says it is not serving the stray folder: {stderr}"
    );

    let backend = GrpcMcpBackend::connect_unix(&socket, None)
        .await
        .expect("connect to the global socket");
    let roots: Vec<String> = backend
        .list_workspaces()
        .await
        .expect("list workspaces")
        .into_iter()
        .map(|w| w.root)
        .collect();
    assert!(
        roots.iter().all(|r| !r.ends_with("client-acme-nda")),
        "the stray folder never reached the registry (and so never outbound_offers): {roots:?}"
    );
    assert!(
        !roots.is_empty(),
        "the default workspace is still there: {roots:?}"
    );
}
