//! `Apply { Add }` on a list that does not exist yet but whose directory does (task
//! desktop-sublist-start): the first line of a sub-list arrives over gRPC right after `RefDir {
//! ensure }` claimed the directory, with no client writing the file. Same in-process harness as
//! `tests/notes_grpc.rs`, copied rather than shared (this crate's own convention).
#![allow(clippy::expect_used, clippy::unwrap_used)]
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

async fn connect(socket: PathBuf) -> Client {
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

async fn serve(root: &Path) -> (Client, tokio::sync::oneshot::Sender<()>) {
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
    for _ in 0..200 {
        if socket.exists() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    (connect(socket).await, stop_tx)
}

fn add(path: &str, line: &str) -> pb::ApplyRequest {
    pb::ApplyRequest {
        path: path.into(),
        mutations: vec![pb::Mutation {
            kind: Some(mutation::Kind::Add(pb::Add { line: line.into() })),
        }],
        ..pb::ApplyRequest::default()
    }
}

#[tokio::test]
async fn the_first_add_to_a_fresh_ref_dirs_list_registers_it() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("todo.txt"), "plan the roadmap\n").unwrap();
    let (mut client, _stop) = serve(dir.path()).await;

    let info = client
        .ref_dir(pb::RefDirRequest {
            path: "todo.txt".into(),
            task: Some(pb::TaskRef {
                line_number: 1,
                task_id: String::new(),
            }),
            ensure: true,
            workspace: None,
        })
        .await
        .unwrap()
        .into_inner();
    let sub = format!("{}/todo.txt", info.dir);
    assert!(dir.path().join(&info.dir).is_dir());
    assert!(!dir.path().join(&sub).exists(), "ensure makes the dir only");

    let applied = client
        .apply(add(&sub, "draft the outline"))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(applied.applied, 1);
    let bytes = client
        .get_file(pb::GetFileRequest {
            path: sub.clone(),
            workspace: None,
        })
        .await
        .unwrap()
        .into_inner()
        .bytes;
    assert!(String::from_utf8_lossy(&bytes).starts_with("draft the outline"));
    let on_disk = std::fs::read_to_string(dir.path().join(&sub)).unwrap();
    assert!(on_disk.starts_with("draft the outline"), "{on_disk}");
    let listed = client
        .list_files(pb::ListFilesRequest::default())
        .await
        .unwrap()
        .into_inner();
    assert!(listed.files.iter().any(|f| f.path == sub), "{listed:?}");
}

#[tokio::test]
async fn an_add_to_a_list_whose_directory_does_not_exist_is_still_not_found() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("todo.txt"), "").unwrap();
    let (mut client, _stop) = serve(dir.path()).await;

    let err = client
        .apply(add("nowhere/todo.txt", "typo"))
        .await
        .unwrap_err();
    assert_eq!(err.code(), tonic::Code::NotFound, "{err}");
    assert!(!dir.path().join("nowhere").exists());
}

/// Root todo "peek_line rejects a line-0-plus-id TaskRef": a cross-file `Move` that names the
/// task by id alone (line 0, the form `todo_move`/`todo_batch` send) now finds its line — here in
/// tagged mode, from the text.
#[tokio::test]
async fn a_cross_file_move_addressed_by_id_alone_works() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("todo.txt"),
        "keep me id:01ARZ3NDEKTSV4RRFFQ69G5FA1\nmove me id:01ARZ3NDEKTSV4RRFFQ69G5FA2\n",
    )
    .unwrap();
    std::fs::create_dir_all(dir.path().join("tasks/other")).unwrap();
    std::fs::write(dir.path().join("tasks/other/todo.txt"), "already here\n").unwrap();
    let (mut client, _stop) = serve(dir.path()).await;

    let req = pb::ApplyRequest {
        path: "todo.txt".into(),
        mutations: vec![pb::Mutation {
            kind: Some(mutation::Kind::Move(pb::Move {
                task: Some(pb::TaskRef {
                    line_number: 0,
                    task_id: "01ARZ3NDEKTSV4RRFFQ69G5FA2".into(),
                }),
                to_path: "tasks/other/todo.txt".into(),
            })),
        }],
        ..pb::ApplyRequest::default()
    };
    client
        .apply(req)
        .await
        .unwrap_or_else(|e| panic!("move by id: {e}"));
    let root = std::fs::read_to_string(dir.path().join("todo.txt")).unwrap();
    let other = std::fs::read_to_string(dir.path().join("tasks/other/todo.txt")).unwrap();
    assert!(!root.contains("move me"), "{root}");
    assert!(other.contains("move me"), "{other}");
}
