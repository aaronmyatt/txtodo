//! Real, two-workspace proof for mcp-multi-workspace-gateway: a real `txtodod` process, started in
//! true global mode (`--dir` omitted — ADR 0025), with two workspaces this test registers+opens by
//! naming their paths in a `workspace` selector (the same auto-register-on-unknown-path behavior
//! `workspace_catalog.rs::resolve`'s doc describes) — exactly the path a fresh agent takes the
//! first time it names a workspace it has never seen before. Drives [`GrpcMcpBackend`] directly
//! (the exact backend `schema.rs`'s tool methods delegate to; going through the full MCP
//! stdio/HTTP transport on top would only add JSON-RPC framing noise around the same call path).
//!
//! `#[ignore]`d and not wired into the automated gate (same precedent as `txtodo-daemon`'s
//! `tests/idle_rss.rs`/`tests/lan_sync_bench.rs`): it spawns a real, separately-built `txtodod`
//! binary this crate cannot depend on (`budgets.json`'s `allowedDeps` — a dev-dependency on
//! `txtodo-daemon` would trip `check-boundaries.sh` exactly like a normal one), so it needs
//! `cargo build -p txtodo-daemon --bin txtodod` to have already produced
//! `target/debug/txtodod` before running (`cargo test -p txtodo-mcp -- --ignored`).

use std::path::{Path, PathBuf};
use std::time::Duration;

use txtodo_mcp::backend::{ListArgs, McpBackend, WorkspaceInfo};
use txtodo_mcp::grpc_backend::GrpcMcpBackend;

/// `target/debug/txtodod`, resolved from this crate's own manifest dir (two levels up is the
/// workspace root — `crates/txtodo-mcp` → `crates` → root) since Cargo's `CARGO_BIN_EXE_<name>`
/// only covers binaries in the *same* package as the test (`txtodo-daemon`'s own tests use it for
/// exactly that reason; this crate cannot). No `#[test]` attribute on this fn, so clippy's
/// "allow expect in tests" heuristic does not reach it — a manual panic instead.
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

/// Kills the spawned `txtodod` even if an assertion later panics — an orphaned daemon holding this
/// tempdir's socket open would otherwise outlive the test.
struct KillOnDrop(std::process::Child);
impl Drop for KillOnDrop {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Waits (up to 5s) for `path` to exist — the daemon binds its socket asynchronously after a
/// startup sequence (registry → catalog → pid lock → gRPC), so a fixed sleep would be both slower
/// than necessary on a fast machine and flaky on a loaded one.
async fn wait_for_socket(path: &Path) {
    for _ in 0..50 {
        if path.exists() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("socket {path:?} never appeared");
}

/// Seeds two empty workspace directories (the daemon's walker only builds a `FileActor` for a
/// document that already exists on disk, plan §3.2's discovery rule — an empty `todo.txt` seeds
/// each one the same way `txtodo init` would), spawns a real `txtodod` in true global mode against
/// them (`TXTODO_SOCKET`/`TXTODO_REGISTRY_DB` isolated to `tmp`), and waits for its socket.
/// `clippy::expect_used` is allowed explicitly, same as `tests/smoke.rs::connect`'s own precedent:
/// clippy's "allow in tests" heuristic only reaches `#[test]`-attributed functions, not a plain
/// helper an integration test calls.
#[allow(clippy::expect_used)]
async fn start_isolated_daemon(tmp: &Path) -> (KillOnDrop, PathBuf, PathBuf, PathBuf) {
    let socket = tmp.join("txtodod.sock");
    let registry_db = tmp.join("registry.db");
    let ws_a = tmp.join("wsA");
    let ws_b = tmp.join("wsB");
    std::fs::create_dir_all(&ws_a).expect("mkdir wsA");
    std::fs::create_dir_all(&ws_b).expect("mkdir wsB");
    std::fs::write(ws_a.join("todo.txt"), "").expect("seed wsA/todo.txt");
    std::fs::write(ws_b.join("todo.txt"), "").expect("seed wsB/todo.txt");

    let daemon = std::process::Command::new(daemon_binary())
        .arg("--identity-mode")
        .arg("tagged")
        .env("TXTODO_SOCKET", &socket)
        .env("TXTODO_REGISTRY_DB", &registry_db)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("spawn txtodod");
    let guard = KillOnDrop(daemon);
    wait_for_socket(&socket).await;
    (guard, socket, ws_a, ws_b)
}

/// `todo_list_workspaces`: both roots must be present, regardless of registration order. Compares
/// by suffix, not exact equality: the daemon's registry stores `root.canonicalize()`'d paths
/// (`workspace_registry.rs::canonical_root`), which can differ from this test's own
/// un-canonicalized `tmp.path()` by symlink resolution (e.g. macOS's `/tmp` → `/private/tmp`) —
/// the `wsA`/`wsB` leaf names are unique enough within one test run either way.
fn assert_both_registered(workspaces: &[WorkspaceInfo], ws_a: &Path, ws_b: &Path) {
    let roots: Vec<&str> = workspaces.iter().map(|w| w.root.as_str()).collect();
    let leaf = |p: &Path| {
        p.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_owned()
    };
    let (a, b) = (leaf(ws_a), leaf(ws_b));
    assert!(
        roots.iter().any(|r| r.ends_with(&a)),
        "workspace A registered: {roots:?}"
    );
    assert!(
        roots.iter().any(|r| r.ends_with(&b)),
        "workspace B registered: {roots:?}"
    );
}

#[tokio::test]
#[ignore = "spawns a real txtodod binary; see module doc for the prerequisite build step"]
async fn workspace_selector_routes_to_the_named_workspace_not_the_other() {
    let tmp = tempfile::tempdir().expect("tempdir");
    // Short path: unix socket paths are capped at SUN_LEN (~104 bytes on macOS/BSD) — a deeply
    // nested tempdir (as this session's own scratchpad is) overflows it, a real failure this test
    // run hit once already. The OS temp dir (what `tempfile::tempdir()` uses) is always short
    // enough.
    let (_daemon_guard, socket, ws_a, ws_b) = start_isolated_daemon(tmp.path()).await;
    let backend = GrpcMcpBackend::connect_unix(&socket, None)
        .await
        .expect("connect to the global socket");
    let (ws_a_str, ws_b_str) = (
        ws_a.to_string_lossy().into_owned(),
        ws_b.to_string_lossy().into_owned(),
    );

    // Naming each workspace by path the first time auto-registers *and* opens it
    // (`workspace_catalog.rs::resolve`'s own doc) — no separate WorkspaceAdd step needed.
    backend
        .add("Task only in A".to_owned(), None, Some(ws_a_str.clone()))
        .await
        .expect("add to workspace A");
    backend
        .add("Task only in B".to_owned(), None, Some(ws_b_str.clone()))
        .await
        .expect("add to workspace B");

    let workspaces = backend
        .list_workspaces()
        .await
        .expect("list_workspaces succeeds");
    assert_both_registered(&workspaces, &ws_a, &ws_b);

    // A selector of A sees only A's task; B's selector sees only B's — proof the selector actually
    // routes, not just that both workspaces happen to exist.
    let rows_a = backend
        .list(ListArgs {
            workspace: Some(ws_a_str),
            ..ListArgs::default()
        })
        .await
        .expect("list workspace A");
    assert_eq!(rows_a.len(), 1, "workspace A has exactly its own task");
    assert!(rows_a[0].raw.contains("Task only in A"));

    let rows_b = backend
        .list(ListArgs {
            workspace: Some(ws_b_str),
            ..ListArgs::default()
        })
        .await
        .expect("list workspace B");
    assert_eq!(rows_b.len(), 1, "workspace B has exactly its own task");
    assert!(rows_b[0].raw.contains("Task only in B"));
}
