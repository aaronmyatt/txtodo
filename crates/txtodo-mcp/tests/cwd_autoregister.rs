//! Task mcp-cwd-autoregister: `txtodo-mcp` started with no flags in a stray folder that happens
//! to hold a `todo.txt` must not register that folder (whose name every paired peer would then be
//! offered — `control_session.rs::outbound_offers` announces every registry entry). It serves the
//! default workspace instead and says so. Real `txtodod` + the real `txtodo-mcp` binary, same
//! `#[ignore]`/prerequisite-build convention as `global_workspace_routing.rs`.
#![allow(clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::time::Duration;

use txtodo_mcp::backend::McpBackend;
use txtodo_mcp::grpc_backend::GrpcMcpBackend;

fn daemon_binary() -> PathBuf {
    let root = match PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
    {
        Ok(p) => p,
        Err(e) => panic!("workspace root did not resolve: {e}"),
    };
    let bin = root.join("target/debug/txtodod");
    assert!(
        bin.exists(),
        "expected {bin:?} to exist — run `cargo build -p txtodo-daemon --bin txtodod` first"
    );
    bin
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

#[tokio::test]
#[ignore = "spawns a real txtodod binary; see module doc for the prerequisite build step"]
async fn a_stray_todo_txt_folder_is_served_as_the_default_and_never_registered() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let socket = tmp.path().join("txtodod.sock");
    let registry_db = tmp.path().join("registry.db");
    let stray = tmp.path().join("client-acme-nda");
    std::fs::create_dir_all(&stray).expect("mkdir stray");
    std::fs::write(stray.join("todo.txt"), "secret client task\n").expect("seed todo.txt");
    let _daemon = KillOnDrop(
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

    // The server with neither --dir nor --global, started inside the stray folder. It aims at
    // the folder only when the daemon already knows it, so it says so on stderr and moves on.
    let mut server = std::process::Command::new(env!("CARGO_BIN_EXE_txtodo-mcp"))
        .arg("--stdio")
        .current_dir(&stray)
        .env("TXTODO_SOCKET", &socket)
        .env("TXTODO_REGISTRY_DB", &registry_db)
        .env("TXTODO_NO_AUTOSTART", "1")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("spawn txtodo-mcp");
    tokio::time::sleep(Duration::from_secs(2)).await;
    let _ = server.kill();
    let output = server.wait_with_output().expect("server output");
    let stderr = String::from_utf8_lossy(&output.stderr);
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
