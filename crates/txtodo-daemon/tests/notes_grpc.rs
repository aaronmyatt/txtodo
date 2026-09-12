//! `GetNotes`/`EditNotes` end to end, in-process, over a real unix socket (plan M5, design §7).
//! Same harness shape as `tests/grpc.rs`; copied, not shared, so this file stays a self-contained
//! read (see `write.rs`'s module doc for the same convention elsewhere in this crate).
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
use txtodo_model::{TaskId, Ulid};
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
    let ws =
        Workspace::open(root, Arc::new(SystemClock)).unwrap_or_else(|e| panic!("workspace: {e}"));
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

async fn apply_add(client: &mut Client, line: &str) -> pb::ApplyResponse {
    let req = pb::ApplyRequest {
        path: "todo.txt".into(),
        mutations: vec![pb::Mutation {
            kind: Some(mutation::Kind::Add(pb::Add { line: line.into() })),
        }],
        agent: None,
    };
    client
        .apply(req)
        .await
        .unwrap_or_else(|e| panic!("apply: {e}"))
        .into_inner()
}

async fn get_todo(client: &mut Client) -> String {
    let bytes = client
        .get_file(pb::GetFileRequest {
            path: "todo.txt".into(),
        })
        .await
        .unwrap_or_else(|e| panic!("get: {e}"))
        .into_inner()
        .bytes;
    String::from_utf8(bytes).unwrap_or_else(|e| panic!("{e}"))
}

/// The task id of a line whose trailing word is its `id:` tag (same convention as `tests/grpc.rs`).
fn task_id_of(line: &str) -> TaskId {
    let id_tag = line.split_whitespace().next_back().expect("has an id tag");
    let id = id_tag.strip_prefix("id:").expect("id tag is last");
    TaskId::new(Ulid::parse(id).expect("valid ulid"))
}

#[tokio::test]
async fn edit_notes_lazily_creates_the_ref_dir_and_get_notes_returns_it() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("todo.txt"), "").unwrap();
    let (mut client, _stop) = serve(dir.path()).await;
    apply_add(&mut client, "buy ducks +farm").await;
    let task = task_id_of(&get_todo(&mut client).await);
    let task_ref = pb::TaskRef {
        line_number: 1,
        task_id: task.to_string(),
    };

    // No `ref:` directory yet: empty doc, no path.
    let empty = client
        .get_notes(task_ref.clone())
        .await
        .unwrap()
        .into_inner();
    assert!(empty.path.is_empty());
    assert!(empty.bytes.is_empty());

    // The first edit creates the `ref:` tag + directory in one op batch, then writes notes.md.
    let applied = client
        .edit_notes(pb::NotesEditRequest {
            task: Some(task_ref.clone()),
            new_text: "# Notes\n\nremember the ducks\n".into(),
        })
        .await
        .unwrap()
        .into_inner();
    assert_eq!(applied.applied, 1);

    let after = get_todo(&mut client).await;
    assert!(after.contains("ref:"), "{after}");

    let doc = client
        .get_notes(task_ref.clone())
        .await
        .unwrap()
        .into_inner();
    assert_eq!(doc.bytes, b"# Notes\n\nremember the ducks\n");
    assert!(doc.path.ends_with("notes.md"), "{}", doc.path);
    assert_eq!(
        doc.bytes,
        std::fs::read(dir.path().join(&doc.path)).unwrap(),
        "disk matches the daemon"
    );

    // A second edit reuses the same `ref:` directory (no second one is created).
    client
        .edit_notes(pb::NotesEditRequest {
            task: Some(task_ref),
            new_text: "# Notes\n\nremember the ducks and the geese\n".into(),
        })
        .await
        .unwrap();
    let second = get_todo(&mut client).await;
    assert_eq!(
        after.matches("ref:").count(),
        second.matches("ref:").count(),
        "still exactly one ref: tag"
    );
}

#[tokio::test]
async fn get_notes_before_any_edit_is_not_an_error() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("todo.txt"), "").unwrap();
    let (mut client, _stop) = serve(dir.path()).await;
    apply_add(&mut client, "buy ducks").await;
    let task = task_id_of(&get_todo(&mut client).await);

    let doc = client
        .get_notes(pb::TaskRef {
            line_number: 1,
            task_id: task.to_string(),
        })
        .await
        .unwrap()
        .into_inner();
    assert_eq!(doc.bytes, Vec::<u8>::new());
    assert_eq!(doc.hash.len(), 32);

    let unknown = TaskId::new(Ulid::from_u128(999));
    let err = client
        .get_notes(pb::TaskRef {
            line_number: 1,
            task_id: unknown.to_string(),
        })
        .await
        .unwrap_err();
    assert_eq!(err.code(), tonic::Code::NotFound);
}
