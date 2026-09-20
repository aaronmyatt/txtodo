//! `GetFile` answers one task id per line (task sidecar-task-ids). Under Sidecar identity a line
//! has no `id:` tag, so the text alone cannot tell a client which id a `TaskRef`, `GetNotes` or
//! `EditNotes` needs; the ids in the reply are the only source. In-process over a temp socket.
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

/// Serves `root` in `mode` (the same in-process shape as `tests/replace_apply.rs`).
async fn serve_in(root: &Path, mode: IdentityMode) -> (Client, tokio::sync::oneshot::Sender<()>) {
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

async fn get_file(client: &mut Client) -> pb::FileContents {
    client
        .get_file(pb::GetFileRequest {
            path: "todo.txt".into(),
            workspace: None,
        })
        .await
        .unwrap()
        .into_inner()
}

#[tokio::test]
async fn sidecar_lines_carry_no_tag_but_get_file_names_every_task() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("todo.txt"), "one\n\ntwo\n").unwrap();
    let (mut client, _stop) = serve_in(dir.path(), IdentityMode::Sidecar).await;

    let file = get_file(&mut client).await;
    let text = String::from_utf8(file.bytes).unwrap();
    assert!(!text.contains("id:"), "sidecar text has no id: tag: {text}");
    assert_eq!(text.lines().count(), 3);
    assert_eq!(
        file.task_ids.len(),
        3,
        "one entry per line, blanks included"
    );
    assert_eq!(file.task_ids[0].len(), 26, "a task line's ULID text");
    assert_eq!(file.task_ids[1], "", "the blank line has no id");
    assert_eq!(file.task_ids[2].len(), 26);
    assert_ne!(file.task_ids[0], file.task_ids[2]);
}

#[tokio::test]
async fn the_id_from_get_file_addresses_the_task_for_apply_and_notes() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("todo.txt"), "one\ntwo\n").unwrap();
    let (mut client, _stop) = serve_in(dir.path(), IdentityMode::Sidecar).await;
    let two = get_file(&mut client).await.task_ids[1].clone();

    // A `TaskRef` whose line number and id agree is accepted: the id is the daemon's own.
    let complete = mutation::Kind::Complete(pb::Complete {
        task: Some(pb::TaskRef {
            line_number: 2,
            task_id: two.clone(),
        }),
        today: "2026-09-20".into(),
    });
    client
        .apply(pb::ApplyRequest {
            path: "todo.txt".into(),
            mutations: vec![pb::Mutation {
                kind: Some(complete),
            }],
            agent: None,
            workspace: None,
        })
        .await
        .unwrap();

    // `EditNotes` resolves by task id alone; before this field a sidecar client had none to send.
    client
        .edit_notes(pb::NotesEditRequest {
            task: Some(pb::TaskRef {
                line_number: 0,
                task_id: two.clone(),
            }),
            new_text: "# two\n".into(),
            workspace: None,
        })
        .await
        .unwrap();
    let notes = client
        .get_notes(pb::GetNotesRequest {
            task: Some(pb::TaskRef {
                line_number: 0,
                task_id: two.clone(),
            }),
            workspace: None,
        })
        .await
        .unwrap()
        .into_inner();
    assert_eq!(notes.bytes, b"# two\n");

    // The id stays with the task across the edit (the line gained `x`, a date and a `ref:`).
    let after = get_file(&mut client).await;
    assert_eq!(after.task_ids[1], two);
    let text = String::from_utf8(after.bytes).unwrap();
    assert!(text.lines().nth(1).unwrap().starts_with("x 2026-09-20 two"));
}

#[tokio::test]
async fn tagged_ids_match_the_tags_in_the_text() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("todo.txt"), "one\ntwo\n").unwrap();
    let (mut client, _stop) = serve_in(dir.path(), IdentityMode::Tagged).await;

    let file = get_file(&mut client).await;
    let text = String::from_utf8(file.bytes).unwrap();
    assert_eq!(file.task_ids.len(), 2);
    for (line, id) in text.lines().zip(&file.task_ids) {
        let tag = line.rsplit_once(" id:").expect("tagged line").1;
        assert_eq!(tag, id);
    }
}
