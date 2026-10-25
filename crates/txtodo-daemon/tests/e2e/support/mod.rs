//! Shared harness for the real-daemon integration tests: spawn `txtodod` on a temp workspace, dial
//! its socket, write files "from outside", wait for it to settle. Slice-local (constitution §7).
#![allow(dead_code)] // each test file uses a different subset of the helpers

/// `MigrateIdentity`/per-task history helpers, split out for this file's own line budget.
pub mod global_socket;
pub mod layout;
pub mod migrate;
/// Global-mode multi-workspace daemon harness, split out for this file's own line budget.
pub mod multi;
#[allow(unused_imports)] // each test file uses a different subset of these re-exports
pub use multi::wait_until_all_open;
/// `ListConflicts` duplicate groups and deletes by id (`duplicate_groups.rs`).
pub mod duplicates;
/// Pairing helpers, split out for this file's own line budget — see its own doc.
pub mod pairing;
/// Relay/forced-relay helpers, split out for this file's own line budget — see its own doc.
pub mod relay;
/// Fixture-writing and pre-daemon state-seeding helpers, split out for this file's line budget.
mod seed;
/// The constructors that seed a sync group and/or workspace id first, split out the same way.
mod starts;

#[allow(unused_imports)] // each test file uses a different subset of these re-exports
pub use seed::{kinds, lines_with_ids, seed_group_id, seed_workspace_id};
use seed::{list_dirs, write_tree};

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
    /// The workspace folder: a list, inside `lists` (`seed::list_dirs`).
    pub dir: tempfile::TempDir,
    _lists: tempfile::TempDir,
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
    /// `start_with_mode(todo, "tagged")`: this harness's whole M3 acceptance suite is about
    /// `id:` tag adoption/stamping, which sidecar mode never does.
    pub async fn start(todo: &str) -> Daemon {
        Self::start_with_mode(todo, "tagged").await
    }

    /// Writes `todo`, starts txtodod under `--identity-mode <mode>`, waits for socket+adoption.
    pub async fn start_with_mode(todo: &str, mode: &str) -> Daemon {
        Self::start_full(todo, mode, &[]).await
    }

    /// `start_with_mode`, but with `TXTODO_TEST_HOOKS=1` set so `DebugSetGroupKey` (plan M4
    /// `sync-lan-transport`) is reachable — the seam a real two-daemon test pairs through.
    pub async fn start_with_test_hooks(todo: &str, mode: &str) -> Daemon {
        Self::start_full(todo, mode, &[("TXTODO_TEST_HOOKS", "1")]).await
    }

    /// `start_with_mode`, with extra environment variables set on the spawned process. Reuse
    /// within this slice (`ABSTRACTIONS.md`'s "real-daemon test harness" entry already tracks this
    /// harness as shared across `tests/e2e/support/mod.rs`/`crash.rs`; this grows it, not duplicates
    /// it — noted there too).
    pub async fn start_full(todo: &str, mode: &str, envs: &[(&str, &str)]) -> Daemon {
        let (lists, dir) = list_dirs();
        write_tree(dir.path(), &[("todo.txt", todo)]);
        Self::start_in(lists, dir, mode, envs, &[]).await
    }

    async fn start_in(
        lists: tempfile::TempDir,
        dir: tempfile::TempDir,
        mode: &str,
        envs: &[(&str, &str)],
        extra_args: &[String],
    ) -> Daemon {
        let mut args: Vec<String> = vec![
            "--dir".into(),
            dir.path().to_string_lossy().into_owned(),
            "--identity-mode".into(),
            mode.into(),
        ];
        // Task relay-default-public-url defaulted txtodod to a public relay; this harness's own
        // default stays offline/fast (`--no-relay`) unless a test's own extra_args already asks
        // for a specific relay (relay_converge.rs/pairing_relay.rs/relay_multiplex.rs).
        if !extra_args.iter().any(|a| a == "--relay") {
            args.push("--no-relay".into());
        }
        let child = Command::new(env!("CARGO_BIN_EXE_txtodod"))
            .args(&args)
            .args(extra_args)
            // Production redials every 15 s (`lan.rs::RESYNC_INTERVAL`); deadlines here assume 1 s.
            .env("TXTODO_RESYNC_INTERVAL_MS", "1000")
            .envs(envs.iter().copied())
            .env("TXTODO_LISTS_DIR", lists.path())
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
            _lists: lists,
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

    /// The real OS pid of the spawned `txtodod` — for out-of-band inspection (`ps`, `/proc`) that
    /// no RPC exposes, e.g. `tests/e2e/idle_rss.rs`'s memory reading.
    pub fn pid(&self) -> u32 {
        self.child.id()
    }

    pub async fn health(&mut self) -> pb::HealthResponse {
        self.client
            .health(pb::HealthRequest { workspace: None })
            .await
            .unwrap_or_else(|e| panic!("health: {e}"))
            .into_inner()
    }

    /// The daemon's bytes for todo.txt.
    pub async fn daemon_bytes(&mut self) -> Vec<u8> {
        let req = pb::GetFileRequest {
            path: "todo.txt".into(),
            workspace: None,
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
            workspace: None,
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
        let cfg_body = format!(
            "export TODO_DIR=\"{dir}\"\nexport TODO_FILE=\"$TODO_DIR/todo.txt\"\nexport DONE_FILE=\"$TODO_DIR/done.txt\"\nexport REPORT_FILE=\"$TODO_DIR/report.txt\"\n"
        );
        std::fs::write(&cfg, cfg_body).unwrap_or_else(|e| panic!("{e}"));
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
