//! Task mcp-dir-second-daemon: the real `txtodod --dir <root>` exits at once, with a message that
//! names the global daemon, when the device's global registry already holds `<root>`, and leaves
//! `<root>/.txtodo/` uncreated (no pid lock, no log, no second store handle). Hermetic: the
//! "global" registry lives under a temp `$XDG_DATA_HOME`/`$HOME`, never the machine's own.
#![allow(clippy::expect_used)]

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use txtodo_daemon::clock::SystemClock;
use txtodo_daemon::workspace_registry::WorkspaceRegistry;

#[test]
fn a_dir_bridge_on_a_root_the_global_registry_holds_exits_before_touching_it() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let base = tmp.path().canonicalize().expect("canonical tmp");
    let root = base.join("ws");
    std::fs::create_dir_all(&root).expect("mkdir root");
    std::fs::write(root.join("todo.txt"), "").expect("seed todo.txt");
    let data = base.join("data");
    let db = data.join("txtodo/registry.db");
    std::fs::create_dir_all(db.parent().expect("parent")).expect("mkdir data");
    WorkspaceRegistry::open(&db)
        .expect("open global registry")
        .add(&root, &SystemClock)
        .expect("register root");

    let mut child = Command::new(env!("CARGO_BIN_EXE_txtodod"))
        .arg("--dir")
        .arg(&root)
        .args(["--no-lan", "--no-relay"])
        .env("XDG_DATA_HOME", &data)
        .env("HOME", &base)
        .env("TXTODO_TEST_KEYSTORE_MEMORY", "1")
        .env("TXTODO_NO_SERVICE", "1")
        .env_remove("TXTODO_REGISTRY_DB")
        .env_remove("TXTODO_SOCKET")
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn txtodod");
    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().expect("poll txtodod") {
            break status;
        }
        if started.elapsed() > Duration::from_secs(60) {
            let _ = child.kill();
            panic!("txtodod --dir kept running on a root the global registry holds");
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    let stderr = std::io::read_to_string(child.stderr.take().expect("stderr")).unwrap_or_default();
    assert!(!status.success(), "refused, not a clean start: {stderr}");
    assert!(
        stderr.contains("global daemon"),
        "the refusal says which daemon owns the root: {stderr}"
    );
    assert!(
        !root.join(".txtodo").exists(),
        "nothing was created under the root before the refusal"
    );
}
