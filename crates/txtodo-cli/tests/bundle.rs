//! `txtodo bundle export|import` end to end (plan M8 `cli-bundle`): two real `txtodod` processes,
//! the real `txtodo` binary, a real socket both ways — proving the CLI's own plumbing (the
//! `--passphrase-file` flag, the on-disk frame format, the gRPC request-metadata passphrase for
//! `BundleImport`) actually works, not just the daemon-side core (`bundle_tests.rs` in
//! `txtodo-daemon` covers the crypto/data-shape guarantees in more depth, in-process).
// Integration tests are tests: clippy.toml allows unwrap/expect in #[test] fns but not in their helpers.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

const SOCKET_WAIT: Duration = Duration::from_secs(20);

fn txtodod_binary() -> PathBuf {
    let mut dir = std::env::current_exe().unwrap_or_else(|e| panic!("current_exe: {e}"));
    dir.pop();
    if dir.ends_with("deps") {
        dir.pop();
    }
    let bin = dir.join(format!("txtodod{}", std::env::consts::EXE_SUFFIX));
    // Non-empty, not just present: apps/desktop's build.rs leaves a 0-byte executable placeholder
    // at target/debug/txtodod (tauri's externalBin copy) which CI's `--exclude txtodo-daemon`
    // never overwrites; exec of it is ENOEXEC on linux and a silent no-op on macOS.
    if !bin.metadata().is_ok_and(|m| m.len() > 0) {
        let status = Command::new(env!("CARGO"))
            .args([
                "build",
                "-p",
                "txtodo-daemon",
                "--bin",
                "txtodod",
                "--quiet",
            ])
            .status()
            .unwrap_or_else(|e| panic!("cargo build txtodod: {e}"));
        assert!(status.success(), "building txtodod failed");
    }
    bin
}

struct Daemon {
    child: Child,
}

impl Daemon {
    fn spawn(dir: &Path) -> Daemon {
        let child = Command::new(txtodod_binary())
            .args(["--dir", &dir.to_string_lossy()])
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap_or_else(|e| panic!("spawn txtodod: {e}"));
        let socket = dir.join(".txtodo").join("txtodod.sock");
        let start = Instant::now();
        while !socket.exists() {
            assert!(
                start.elapsed() < SOCKET_WAIT,
                "daemon socket did not appear"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
        Daemon { child }
    }
}

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn txtodo(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_txtodo"))
        .current_dir(dir)
        .env_remove("TXTODO_TODO_DIR")
        .env("TXTODO_CONFIG", dir.join("none.toml"))
        .args(args)
        .output()
        .unwrap_or_else(|e| panic!("txtodo: {e}"))
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

#[test]
fn export_from_a_and_import_into_fresh_b_over_real_sockets() {
    let dir_a = tempfile::tempdir().unwrap();
    let dir_b = tempfile::tempdir().unwrap();
    std::fs::write(dir_a.path().join("todo.txt"), "").unwrap();
    std::fs::write(dir_b.path().join("todo.txt"), "").unwrap();
    let passphrase_file = dir_a.path().join("passphrase.txt");
    std::fs::write(&passphrase_file, "correct horse battery staple\n").unwrap();

    let _daemon_a = Daemon::spawn(dir_a.path());
    let add = txtodo(dir_a.path(), &["add", "buy", "ducks", "+farm"]);
    assert!(add.status.success(), "{}", stderr(&add));

    let bundle_path = dir_a.path().join("out.txtodo");
    let export = txtodo(
        dir_a.path(),
        &[
            "bundle",
            "export",
            "--out",
            &bundle_path.to_string_lossy(),
            "--passphrase-file",
            &passphrase_file.to_string_lossy(),
        ],
    );
    assert!(export.status.success(), "{}", stderr(&export));
    assert!(bundle_path.exists(), "{}", stdout(&export));

    let _daemon_b = Daemon::spawn(dir_b.path());
    let import = txtodo(
        dir_b.path(),
        &[
            "bundle",
            "import",
            &bundle_path.to_string_lossy(),
            "--passphrase-file",
            &passphrase_file.to_string_lossy(),
        ],
    );
    assert!(import.status.success(), "{}", stderr(&import));
    assert!(stdout(&import).contains("imported"), "{}", stdout(&import));

    let bytes_a = std::fs::read(dir_a.path().join("todo.txt")).unwrap();
    let bytes_b = std::fs::read(dir_b.path().join("todo.txt")).unwrap();
    assert_eq!(bytes_a, bytes_b, "byte-identical over the real socket path");
}
