//! Concurrent `Apply` load: N agents, one tonic connection each (like N separate CLI/MCP
//! processes), first all on one document, then one document each. Asserts nothing is lost,
//! duplicated, reordered or errored, and prints wall-clock so a human can see where time goes:
//!   cargo test -p txtodo-daemon --test concurrent_apply -- --nocapture
//! Also pins the `TaskRef` stale-id guard both ways (id sent → refused; id omitted → wrong task).
//! In-process daemon (a `Workspace` on a temp unix socket), the same harness shape as `grpc.rs`.
// Integration tests are tests: clippy.toml allows unwrap/expect in #[test] fns but not in their helpers.
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::print_stderr)]
// A unix-domain socket is the daemon's only transport (ADR 0010); this cannot run on Windows.
#![cfg(unix)]

use hyper_util::rt::TokioIo;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};
use tokio::net::UnixStream;
use tokio::task::JoinSet;
use tonic::transport::{Channel, Endpoint, Uri};
use tower::service_fn;
use txtodo_daemon::clock::SystemClock;
use txtodo_daemon::workspace::Workspace;
use txtodo_daemon::{serve, server};
use txtodo_model::IdentityMode;
use txtodo_proto::v1::txtodo_client::TxtodoClient;
use txtodo_proto::v1::{self as pb, mutation};

type Client = TxtodoClient<Channel>;

/// Concurrent agents per scenario.
const AGENTS: usize = 8;
/// `Add`s each agent sends, one at a time (a real agent waits for each reply).
const ADDS: usize = 10;
/// A deadlock or a wedged actor fails the test instead of hanging it.
const HANG_GUARD: Duration = Duration::from_secs(60);

async fn connect(socket: PathBuf) -> Client {
    // The URI is required by tonic but ignored by the connector.
    // https://github.com/hyperium/tonic/tree/master/examples/src/uds
    let channel = Endpoint::try_from("http://[::]:50051")
        .unwrap_or_else(|e| panic!("{e}"))
        .connect_with_connector(service_fn(move |_: Uri| {
            let socket = socket.clone();
            async move { UnixStream::connect(socket).await.map(TokioIo::new) }
        }))
        .await
        .unwrap_or_else(|e| panic!("connect: {e}"));
    TxtodoClient::new(channel)
}

/// Serves `root` on a socket inside it; the server task ends when the returned sender drops.
/// Tagged mode, like `grpc.rs`: the stale-id guard only exists there.
async fn serve_root(root: &Path) -> (PathBuf, tokio::sync::oneshot::Sender<()>) {
    let ws = Workspace::open_with_default_mode(root, Arc::new(SystemClock), IdentityMode::Tagged)
        .unwrap_or_else(|e| panic!("workspace: {e}"));
    let ws: server::SharedWorkspace = Arc::new(RwLock::new(ws));
    let socket = root.join(".txtodo").join("txtodod.sock");
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
    let sock = socket.clone();
    tokio::spawn(async move {
        let shutdown = async {
            let _ = stop_rx.await;
        };
        serve::serve(ws, &sock, shutdown)
            .await
            .unwrap_or_else(|e| panic!("serve: {e}"));
    });
    // Bounded wait for the socket file (the server task binds it first thing).
    for _ in 0..200 {
        if socket.exists() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    (socket, stop_tx)
}

fn seed(root: &Path, rel: &str, text: &str) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn mutation_req(path: &str, kind: mutation::Kind) -> pb::ApplyRequest {
    pb::ApplyRequest {
        path: path.into(),
        mutations: vec![pb::Mutation { kind: Some(kind) }],
        agent: None,
        workspace: None,
        ..pb::ApplyRequest::default()
    }
}

/// One agent: its own connection, `adds` sequential `Add`s to `path`; returns each call's latency.
async fn agent(socket: PathBuf, path: String, tag: usize, adds: usize) -> Vec<Duration> {
    let mut client = connect(socket).await;
    let mut latencies = Vec::with_capacity(adds);
    for i in 0..adds {
        let line = format!("agent{tag} task{i}");
        let req = mutation_req(&path, mutation::Kind::Add(pb::Add { line }));
        let started = Instant::now();
        let reply = client.apply(req).await;
        reply.unwrap_or_else(|e| panic!("agent {tag} add {i} on {path}: {e}"));
        latencies.push(started.elapsed());
    }
    latencies
}

struct Run {
    wall: Duration,
    latencies: Vec<Duration>,
}

/// One agent per entry of `paths` (its index is the agent's tag), all started together.
/// https://docs.rs/tokio/latest/tokio/task/struct.JoinSet.html
async fn run_agents(socket: &Path, paths: Vec<String>, adds: usize) -> Run {
    let started = Instant::now();
    let mut set = JoinSet::new();
    for (tag, path) in paths.into_iter().enumerate() {
        set.spawn(agent(socket.to_path_buf(), path, tag, adds));
    }
    let mut latencies = Vec::new();
    // Bounded: exactly one result per spawned agent.
    while let Some(done) = set.join_next().await {
        latencies.extend(done.unwrap());
    }
    Run {
        wall: started.elapsed(),
        latencies,
    }
}

/// `run_agents` under `HANG_GUARD`: https://docs.rs/tokio/latest/tokio/time/fn.timeout.html
async fn timed(label: &str, socket: &Path, paths: Vec<String>, adds: usize) -> Run {
    let mut run = tokio::time::timeout(HANG_GUARD, run_agents(socket, paths, adds))
        .await
        .unwrap_or_else(|_| panic!("{label}: no result in {HANG_GUARD:?} (deadlock?)"));
    run.latencies.sort();
    let n = run.latencies.len();
    let (p50, max) = (run.latencies[n / 2], run.latencies[n - 1]);
    let per_sec = n as f64 / run.wall.as_secs_f64();
    eprintln!(
        "{label:<11} {n} applies in {:>8.1?} = {per_sec:>6.0}/s   p50 {p50:.1?}  max {max:.1?}",
        run.wall
    );
    run
}

/// `agent{tag} task{i}` → `Some(i)`; anything else → `None`.
fn task_index(line: &str, tag: usize) -> Option<usize> {
    let rest = line.strip_prefix(&format!("agent{tag} task"))?;
    rest.split(' ').next()?.parse().ok()
}

/// Every add landed exactly once with its own id, and each agent's adds kept their send order.
fn assert_doc(root: &Path, rel: &str, tags: &[usize], adds: usize) {
    let text = std::fs::read_to_string(root.join(rel)).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(
        lines.len(),
        1 + tags.len() * adds,
        "{rel}: lost or duplicated adds:\n{text}"
    );
    let ids: HashSet<&str> = lines
        .iter()
        .map(|l| l.rsplit_once(" id:").expect("id tag").1)
        .collect();
    assert_eq!(ids.len(), lines.len(), "{rel}: duplicate ids");
    for &tag in tags {
        let seen: Vec<usize> = lines.iter().filter_map(|l| task_index(l, tag)).collect();
        assert_eq!(
            seen,
            (0..adds).collect::<Vec<_>>(),
            "{rel}: agent {tag} out of order"
        );
    }
}

/// 4 worker threads: `txtodod` itself uses `Runtime::new()` (one per core), so a single-thread
/// test runtime would serialize the actors' blocking SQLite/fsync calls and mislead.
/// https://docs.rs/tokio/latest/tokio/attr.test.html
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn parallel_agents_lose_nothing_and_report_timing() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    seed(root, "todo.txt", "seed\n");
    seed(root, "tasks/base/todo.txt", "seed\n");
    let own: Vec<String> = (0..AGENTS)
        .map(|i| format!("tasks/a{i}/todo.txt"))
        .collect();
    for rel in &own {
        seed(root, rel, "seed\n");
    }
    let (socket, _stop) = serve_root(root).await;
    let total = AGENTS * ADDS;

    // Baseline: one agent does all the work, one document. No contention possible.
    timed(
        "sequential",
        &socket,
        vec!["tasks/base/todo.txt".into()],
        total,
    )
    .await;
    assert_doc(root, "tasks/base/todo.txt", &[0], total);

    // Same document: every agent queues behind the one actor.
    timed(
        "same file",
        &socket,
        vec!["todo.txt".to_owned(); AGENTS],
        ADDS,
    )
    .await;
    assert_doc(root, "todo.txt", &(0..AGENTS).collect::<Vec<_>>(), ADDS);

    // One document each: actors run side by side, sharing only the store's mutex.
    timed("own file", &socket, own.clone(), ADDS).await;
    for (tag, rel) in own.iter().enumerate() {
        assert_doc(root, rel, &[tag], ADDS);
    }
}

fn id_of(line: &str) -> String {
    line.rsplit_once(" id:").expect("id tag").1.to_owned()
}

fn complete_req(line_number: u32, task_id: &str) -> pb::ApplyRequest {
    let task = Some(pb::TaskRef {
        line_number,
        task_id: task_id.into(),
    });
    let kind = mutation::Kind::Complete(pb::Complete {
        task,
        today: "2026-09-19".into(),
    });
    mutation_req("todo.txt", kind)
}

/// Agent B read the file and remembers `three` at line 3. Agent A then deletes line 1, so line 3
/// is now `four`. With the id, B is refused; without it, B silently completes the wrong task.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stale_line_number_is_refused_only_when_the_id_is_sent() {
    let dir = tempfile::tempdir().unwrap();
    seed(dir.path(), "todo.txt", "one\ntwo\nthree\nfour\n");
    let (socket, _stop) = serve_root(dir.path()).await;
    let (mut a, mut b) = (connect(socket.clone()).await, connect(socket).await);
    let text = std::fs::read_to_string(dir.path().join("todo.txt")).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    let (one, three) = (id_of(lines[0]), id_of(lines[2]));

    let delete = pb::Delete {
        task: Some(pb::TaskRef {
            line_number: 1,
            task_id: one,
        }),
        leave_blank: false,
    };
    a.apply(mutation_req("todo.txt", mutation::Kind::Delete(delete)))
        .await
        .unwrap();

    let err = b.apply(complete_req(3, &three)).await.unwrap_err();
    assert_eq!(err.code(), tonic::Code::FailedPrecondition, "{err}");
    let after = std::fs::read_to_string(dir.path().join("todo.txt")).unwrap();
    assert!(
        !after.lines().any(|l| l.starts_with("x ")),
        "refused, nothing completed:\n{after}"
    );

    b.apply(complete_req(3, "")).await.unwrap();
    let after = std::fs::read_to_string(dir.path().join("todo.txt")).unwrap();
    let done: Vec<&str> = after.lines().filter(|l| l.starts_with("x ")).collect();
    assert_eq!(done.len(), 1, "{after}");
    assert!(
        done[0].contains("four"),
        "no id sent: the wrong task got completed:\n{after}"
    );
    assert!(after.lines().any(|l| l.starts_with("three ")), "{after}");
}
