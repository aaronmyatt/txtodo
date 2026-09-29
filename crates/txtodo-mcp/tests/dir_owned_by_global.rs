//! Task mcp-dir-second-daemon: `txtodo-mcp --dir <folder>` (what `txtodo mcp` always runs) on a
//! folder the live global daemon already owns must serve it through that daemon, never start a
//! `txtodod --dir` bridge on the same op log and files. On 2026-09-28 it did: every write landed
//! twice in the log and both daemons fought over one relay endpoint id.
//!
//! Autostart is off, so before the fix this fails loudly (the server dials the bridge socket
//! nobody serves and exits) instead of spawning a second daemon from a test. Real `txtodod` +
//! the real `txtodo-mcp` binary, same `#[ignore]`/prerequisite-build convention as
//! `cwd_autoregister.rs`: run `cargo build -p txtodo-daemon --bin txtodod` first.
#![allow(clippy::expect_used)]

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use txtodo_mcp::backend::McpBackend;
use txtodo_mcp::grpc_backend::GrpcMcpBackend;

fn daemon_binary() -> PathBuf {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("workspace root");
    let bin = root.join("target/debug/txtodod");
    assert!(
        bin.exists(),
        "expected {bin:?} to exist — run `cargo build -p txtodo-daemon --bin txtodod` first"
    );
    bin
}

struct KillOnDrop(Child);
impl Drop for KillOnDrop {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

async fn wait_for_socket(path: &Path) {
    for _ in 0..300 {
        if path.exists() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("socket {path:?} never appeared");
}

/// One newline-delimited JSON-RPC request, and the reply carrying its id.
/// Ref: <https://modelcontextprotocol.io/specification/2025-06-18/basic/transports#stdio>
fn call(child: &mut Child, out: &mut impl BufRead, id: u32, method: &str, params: &str) -> String {
    let stdin = child.stdin.as_mut().expect("stdin");
    writeln!(
        stdin,
        r#"{{"jsonrpc":"2.0","id":{id},"method":"{method}","params":{params}}}"#
    )
    .expect("write request");
    let want = format!(r#""id":{id}"#);
    let mut line = String::new();
    loop {
        line.clear();
        let n = out.read_line(&mut line).expect("read reply");
        assert!(n > 0, "txtodo-mcp closed stdout before answering {method}");
        if line.contains(&want) {
            return line;
        }
    }
}

/// Serves `dir` with `txtodo-mcp --dir`, lists its tasks, and returns the list reply and stderr.
fn list_through_mcp(dir: &Path, socket: &Path, registry: &Path) -> (String, String) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_txtodo-mcp"))
        .arg("--dir")
        .arg(dir)
        .arg("--stdio")
        .env("TXTODO_SOCKET", socket)
        .env("TXTODO_REGISTRY_DB", registry)
        .env("TXTODO_NO_AUTOSTART", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn txtodo-mcp");
    let mut out = BufReader::new(child.stdout.take().expect("stdout"));
    let init = r#"{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"t","version":"0"}}"#;
    call(&mut child, &mut out, 1, "initialize", init);
    let listed = call(
        &mut child,
        &mut out,
        2,
        "tools/call",
        r#"{"name":"todo_list","arguments":{}}"#,
    );
    drop(child.stdin.take());
    let _ = child.kill();
    let output = child.wait_with_output().expect("txtodo-mcp output");
    (listed, String::from_utf8_lossy(&output.stderr).into_owned())
}

#[tokio::test]
#[ignore = "spawns a real txtodod binary; see module doc for the prerequisite build step"]
async fn dir_on_a_root_the_global_daemon_owns_is_served_through_it() {
    // Short path: unix socket paths are capped near 104 bytes on macOS.
    let tmp = tempfile::tempdir().expect("tempdir");
    let socket = tmp.path().join("g.sock");
    let registry = tmp.path().join("registry.db");
    let root = tmp.path().join("ws");
    std::fs::create_dir_all(root.join("tasks/sub")).expect("mkdir");
    std::fs::write(root.join("todo.txt"), "").expect("seed todo.txt");
    let root = root.canonicalize().expect("canonical root");
    let _daemon = KillOnDrop(
        Command::new(daemon_binary())
            .args(["--no-lan", "--no-relay"])
            .env("TXTODO_SOCKET", &socket)
            .env("TXTODO_REGISTRY_DB", &registry)
            .env("TXTODO_TEST_KEYSTORE_MEMORY", "1")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn txtodod"),
    );
    wait_for_socket(&socket).await;
    // Naming the folder by path registers and opens it in the global daemon.
    let backend = GrpcMcpBackend::connect_unix(&socket, None)
        .await
        .expect("connect to the global socket");
    backend
        .add(
            "written through the global daemon".to_owned(),
            None,
            Some(root.display().to_string()),
        )
        .await
        .expect("add through the global daemon");

    for dir in [root.clone(), root.join("tasks/sub")] {
        let (listed, stderr) = list_through_mcp(&dir, &socket, &registry);
        assert!(
            listed.contains("written through the global daemon"),
            "--dir {dir:?} lists the global daemon's copy of the root: {listed}"
        );
        assert!(
            stderr.contains("serving it there"),
            "stderr says where it is served: {stderr}"
        );
        assert!(
            !dir.join(".txtodo/txtodod.sock").exists(),
            "no --dir bridge socket beside {dir:?}"
        );
    }
}
