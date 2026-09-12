//! Shared harness for the real-daemon integration tests: spawn `txtodod` on a temp workspace,
//! dial its socket, write files "from outside", and wait for the daemon to settle. Slice-local by
//! design (constitution §7: no cross-slice helpers).
#![allow(dead_code)] // each test file uses a different subset of the helpers

use hyper_util::rt::TokioIo;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};
use tokio::net::UnixStream;
use tonic::transport::{Channel, Endpoint, Uri};
use tower::service_fn;
use txtodo_proto::v1::txtodo_client::TxtodoClient;
use txtodo_proto::v1::{self as pb};

/// How long to wait for the socket after spawn. Generous: a debug `txtodod` adopts the whole
/// workspace before it binds and CI runners are slow (20 s timed out on ubuntu 2026-09-12).
pub const SOCKET_WAIT: Duration = Duration::from_secs(120);
/// How long the daemon gets to notice and finish reconciling an external write. Bounded, above the
/// debug-build reconcile cost the slow runners add.
pub const SETTLE_TIMEOUT: Duration = Duration::from_secs(30);
/// Quiet time (no hash change) before an external edit counts as settled; above the 150 ms debounce.
pub const QUIET_MS: u64 = 400;

pub type Client = TxtodoClient<Channel>;

/// A running daemon on its own temp dir, with a client and the baselines taken after adoption.
pub struct Daemon {
    pub dir: tempfile::TempDir,
    child: Child,
    client: Client,
    writes_baseline: u64,
    seq_baseline: i64,
}

/// Dials the socket; a daemon that has bound but not yet accepted answers a few hundred ms later,
/// so this retries (bounded) instead of failing the whole scenario on a slow test machine.
async fn connect(socket: PathBuf) -> Client {
    let start = Instant::now();
    let mut attempt = 0u32;
    loop {
        attempt += 1;
        let dial = socket.clone();
        let result = Endpoint::try_from("http://[::]:50051")
            .unwrap_or_else(|e| panic!("{e}"))
            .connect_with_connector(service_fn(move |_: Uri| {
                let dial = dial.clone();
                async move { UnixStream::connect(dial).await.map(TokioIo::new) }
            }))
            .await;
        match result {
            Ok(channel) => return TxtodoClient::new(channel),
            Err(e) => {
                assert!(
                    start.elapsed() < SOCKET_WAIT,
                    "connect failed after {attempt} attempts: {e}"
                );
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }
    }
}

impl Daemon {
    /// Writes `todo` as todo.txt, starts txtodod, waits for the socket and for adoption to settle.
    pub async fn start(todo: &str) -> Daemon {
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
        std::fs::write(dir.path().join("todo.txt"), todo).unwrap_or_else(|e| panic!("{e}"));
        let child = Command::new(env!("CARGO_BIN_EXE_txtodod"))
            .args(["--dir", &dir.path().to_string_lossy()])
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap_or_else(|e| panic!("spawn txtodod: {e}"));
        let socket = dir.path().join(".txtodo").join("txtodod.sock");
        let start = Instant::now();
        while !socket.exists() {
            assert!(start.elapsed() < SOCKET_WAIT, "socket did not appear");
            std::thread::sleep(Duration::from_millis(20));
        }
        let client = connect(socket).await;
        let mut d = Daemon {
            dir,
            child,
            client,
            writes_baseline: 0,
            seq_baseline: 0,
        };
        d.settle().await;
        let health = d.health().await;
        d.writes_baseline = health.writes_total;
        d.seq_baseline = d.raw_history().await.first().map_or(0, |o| o.seq);
        d
    }

    pub async fn health(&mut self) -> pb::HealthResponse {
        self.client
            .health(pb::HealthRequest {})
            .await
            .unwrap_or_else(|e| panic!("health: {e}"))
            .into_inner()
    }

    /// The daemon's bytes for todo.txt.
    pub async fn daemon_bytes(&mut self) -> Vec<u8> {
        let req = pb::GetFileRequest {
            path: "todo.txt".into(),
        };
        self.client
            .get_file(req)
            .await
            .unwrap_or_else(|e| panic!("get_file: {e}"))
            .into_inner()
            .bytes
    }

    /// Bytes on disk.
    pub fn disk(&self) -> String {
        self.disk_file("todo.txt")
    }

    pub fn disk_file(&self, name: &str) -> String {
        String::from_utf8(std::fs::read(self.dir.path().join(name)).unwrap_or_default())
            .unwrap_or_default()
    }

    /// Diagnostic only: the daemon's own JSON log, whatever rotation file(s) exist.
    pub fn log_tail(&self) -> String {
        let dir = self.dir.path().join(".txtodo").join("logs");
        let Ok(entries) = std::fs::read_dir(&dir) else {
            return format!("(no log dir at {})", dir.display());
        };
        let mut out = String::new();
        for entry in entries.flatten() {
            out.push_str(&format!("--- {} ---\n", entry.path().display()));
            out.push_str(&std::fs::read_to_string(entry.path()).unwrap_or_default());
        }
        out
    }

    /// An editor-style save: write the whole file in place (truncate + write).
    pub fn external_write(&self, text: &str) {
        std::fs::write(self.dir.path().join("todo.txt"), text).unwrap_or_else(|e| panic!("{e}"));
    }

    /// Waits until the daemon's bytes equal the disk bytes and stayed unchanged for `QUIET_MS`.
    /// Returns the disk text.
    pub async fn settle(&mut self) -> String {
        let start = Instant::now();
        let mut stable_since: Option<Instant> = None;
        let mut last: Option<Vec<u8>> = None;
        // Bounded by SETTLE_TIMEOUT.
        loop {
            let daemon = self.daemon_bytes().await;
            let disk = std::fs::read(self.dir.path().join("todo.txt")).unwrap_or_default();
            let daemon_debug = String::from_utf8_lossy(&daemon).into_owned();
            if daemon == disk && last.as_ref() == Some(&daemon) {
                let since = *stable_since.get_or_insert_with(Instant::now);
                if since.elapsed() >= Duration::from_millis(QUIET_MS) {
                    return String::from_utf8(disk).unwrap_or_default();
                }
            } else {
                stable_since = None;
                last = Some(daemon);
            }
            assert!(
                start.elapsed() < SETTLE_TIMEOUT,
                "daemon did not settle: disk={:?}\ndaemon={:?}\nlog={}",
                String::from_utf8_lossy(&disk),
                daemon_debug,
                self.log_tail()
            );
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    }

    /// Projection writes since the baseline taken after adoption.
    pub async fn writes_since(&mut self) -> u64 {
        self.health().await.writes_total - self.writes_baseline
    }

    async fn raw_history(&mut self) -> Vec<pb::OpSummary> {
        let req = pb::HistoryRequest {
            path: "todo.txt".into(),
            task_id: String::new(),
            limit: 1000,
            before_seq: 0,
        };
        self.client
            .history(req)
            .await
            .unwrap_or_else(|e| panic!("history: {e}"))
            .into_inner()
            .ops
    }

    /// Ops recorded since the baseline, oldest first.
    pub async fn history(&mut self) -> Vec<pb::OpSummary> {
        let mut ops: Vec<pb::OpSummary> = self
            .raw_history()
            .await
            .into_iter()
            .filter(|o| o.seq > self.seq_baseline)
            .collect();
        ops.reverse();
        ops
    }

    /// Runs the vendored todo.sh (the CLI slice's test oracle, invoked by path) on this workspace.
    pub fn todo_sh(&self, args: &[&str]) -> Output {
        let script =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../txtodo-cli/tests/vendor/todo.sh");
        assert!(script.exists(), "vendored todo.sh at {}", script.display());
        let cfg = self.dir.path().join("todo.cfg");
        let dir = self.dir.path().to_string_lossy().into_owned();
        std::fs::write(
            &cfg,
            format!("export TODO_DIR=\"{dir}\"\nexport TODO_FILE=\"$TODO_DIR/todo.txt\"\nexport DONE_FILE=\"$TODO_DIR/done.txt\"\nexport REPORT_FILE=\"$TODO_DIR/report.txt\"\n"),
        )
        .unwrap_or_else(|e| panic!("{e}"));
        Command::new("bash")
            .arg(script)
            .args(["-d", &cfg.to_string_lossy(), "-f", "-p"])
            .args(args)
            .current_dir(self.dir.path())
            .output()
            .unwrap_or_else(|e| panic!("todo.sh: {e}"))
    }
}

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// The `kind` column of each op, in order.
pub fn kinds(ops: &[pb::OpSummary]) -> Vec<String> {
    ops.iter().map(|o| o.kind.clone()).collect()
}

/// The lines of a file as owned strings (so tests can splice), without endings.
pub fn lines_with_ids(text: &str) -> Vec<String> {
    let lines: Vec<String> = text.lines().map(str::to_owned).collect();
    assert!(
        lines.iter().all(|l| l.is_empty() || l.contains(" id:")),
        "every task line carries an id after adoption"
    );
    lines
}
