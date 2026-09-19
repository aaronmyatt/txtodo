//! Where the daemon puts things, for every single mutation on every small sidecar file (up to 3
//! lines over `a`, `b` and a blank). `txtodo-cli`'s `plan_check` replays its plans on exactly this
//! model to decide whether a plan reproduces the file the command printed item numbers for, and a
//! plan is only sent if it does, so this is the contract that replay leans on:
//! - `Add` and `MoveToEnd` put the task right after the last non-blank line, before any trailing
//!   blank (direct mode appends after the blanks, which is where the two differ);
//! - `Delete` removes the line, or with `leave_blank` leaves a blank where it was;
//! - `Edit` replaces the line's text.
//!
//! If one of these changes, the CLI's replay has to change with it.
// Integration tests are tests: clippy.toml allows unwrap/expect in #[test] fns but not in their helpers.
#![allow(clippy::expect_used, clippy::unwrap_used)]
// A unix-domain socket is the daemon's only transport (ADR 0010); this cannot run on Windows.
#![cfg(unix)]

use hyper_util::rt::TokioIo;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use tokio::net::UnixStream;
use tonic::transport::{Channel, Endpoint, Uri};
use tower::service_fn;
use txtodo_daemon::clock::SystemClock;
use txtodo_daemon::workspace::Workspace;
use txtodo_daemon::{serve, server};
use txtodo_model::IdentityMode;
use txtodo_proto::v1::txtodo_client::TxtodoClient;
use txtodo_proto::v1::{self as pb, mutation};

type Client = TxtodoClient<Channel>;
type Lines = Vec<String>;

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

/// Serves `root` in sidecar mode (no `id:` in the text, so lines compare as plain strings).
async fn serve_sidecar(root: &Path) -> (Client, tokio::sync::oneshot::Sender<()>) {
    let mode = IdentityMode::Sidecar;
    let ws = Workspace::open_with_default_mode(root, Arc::new(SystemClock), mode)
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
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    (connect(socket).await, stop_tx)
}

/// A mutation on the 1-based `line` of a file, and the same thing in the model.
#[derive(Clone, Copy, Debug)]
enum Op {
    Add,
    Delete { line: u32, leave_blank: bool },
    MoveToEnd { line: u32 },
    Edit { line: u32 },
}

fn task(line: u32) -> Option<pb::TaskRef> {
    Some(pb::TaskRef {
        line_number: line,
        task_id: String::new(),
    })
}

fn wire(op: Op) -> mutation::Kind {
    match op {
        Op::Add => mutation::Kind::Add(pb::Add { line: "z".into() }),
        Op::Delete { line, leave_blank } => mutation::Kind::Delete(pb::Delete {
            task: task(line),
            leave_blank,
        }),
        Op::MoveToEnd { line } => mutation::Kind::MoveToEnd(pb::MoveToEnd { task: task(line) }),
        Op::Edit { line } => mutation::Kind::Edit(pb::Edit {
            task: task(line),
            new_line: "e".into(),
        }),
    }
}

/// Right after the last non-blank line, or first when there is none.
fn tail(lines: &[String]) -> usize {
    lines
        .iter()
        .rposition(|l| !l.is_empty())
        .map_or(0, |i| i + 1)
}

fn model(seed: &[String], op: Op) -> Lines {
    let mut lines = seed.to_vec();
    match op {
        Op::Add => lines.insert(tail(&lines), "z".into()),
        Op::Delete {
            line,
            leave_blank: true,
        } => lines[line as usize - 1].clear(),
        Op::Delete { line, .. } => drop(lines.remove(line as usize - 1)),
        Op::MoveToEnd { line } => {
            let moved = lines.remove(line as usize - 1);
            lines.insert(tail(&lines), moved);
        }
        Op::Edit { line } => lines[line as usize - 1] = "e".into(),
    }
    lines
}

/// Every mutation that is legal on `seed`: an add, and each of the others on each non-blank line.
fn ops(seed: &[String]) -> Vec<Op> {
    let mut all = vec![Op::Add];
    for line in (1..=seed.len() as u32).filter(|l| !seed[*l as usize - 1].is_empty()) {
        all.push(Op::Delete {
            line,
            leave_blank: false,
        });
        all.push(Op::Delete {
            line,
            leave_blank: true,
        });
        all.push(Op::MoveToEnd { line });
        all.push(Op::Edit { line });
    }
    all
}

/// Every file of up to 3 lines over `a`, `b` and a blank.
fn seeds() -> Vec<Lines> {
    let mut out: Vec<Lines> = vec![Vec::new()];
    let mut layer: Vec<Lines> = vec![Vec::new()];
    for _ in 0..3 {
        layer = layer
            .iter()
            .flat_map(|f| ["a", "b", ""].map(|l| [f.clone(), vec![l.to_owned()]].concat()))
            .collect();
        out.extend(layer.clone());
    }
    out
}

fn text_of(lines: &[String]) -> String {
    if lines.is_empty() {
        String::new()
    } else {
        lines.join("\n") + "\n"
    }
}

async fn read(client: &mut Client) -> (Lines, Vec<u8>) {
    let file = client
        .get_file(pb::GetFileRequest {
            path: "todo.txt".into(),
            workspace: None,
        })
        .await
        .unwrap()
        .into_inner();
    let text = String::from_utf8(file.bytes).unwrap();
    (text.lines().map(str::to_owned).collect(), file.hash)
}

fn request(kind: mutation::Kind) -> pb::ApplyRequest {
    pb::ApplyRequest {
        path: "todo.txt".into(),
        mutations: vec![pb::Mutation { kind: Some(kind) }],
        agent: None,
        workspace: None,
    }
}

#[tokio::test]
async fn the_daemon_places_every_single_mutation_where_the_cli_replay_assumes() {
    let mut wrong = Vec::new();
    for seed in seeds() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("todo.txt"), text_of(&seed)).unwrap();
        let (mut client, _stop) = serve_sidecar(dir.path()).await;
        for op in ops(&seed) {
            client.apply(request(wire(op))).await.unwrap();
            let (got, hash) = read(&mut client).await;
            if got != model(&seed, op) {
                wrong.push(format!(
                    "{seed:?} {op:?}: daemon {got:?}, model {:?}",
                    model(&seed, op)
                ));
            }
            // Back to the seed, guarded by the hash just read, for the next mutation.
            let reset = mutation::Kind::Replace(pb::Replace {
                base_hash: hash,
                contents: text_of(&seed).into_bytes(),
            });
            client.apply(request(reset)).await.unwrap();
            assert_eq!(read(&mut client).await.0, seed, "reset to the seed");
        }
    }
    assert!(
        wrong.is_empty(),
        "{} placements differ:\n{}",
        wrong.len(),
        wrong[..wrong.len().min(6)].join("\n")
    );
}
