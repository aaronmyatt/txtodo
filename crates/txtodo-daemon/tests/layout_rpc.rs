//! The `WorkspaceLayout` RPC over the socket (task `workspace-layout`): read the layout, change it,
//! refuse a change while ref dirs sit in the old place, or move them when asked. In-process daemon,
//! real files.
// Integration tests are tests: clippy.toml allows unwrap/expect in #[test] fns but not in their helpers.
#![allow(clippy::expect_used, clippy::unwrap_used)]
// A unix-domain socket is the daemon's only transport (ADR 0010); this cannot run on Windows.
#![cfg(unix)]

use hyper_util::rt::TokioIo;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use tokio::net::UnixStream;
use tonic::Code;
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

/// Serves `root` in `mode`; no watcher runs in-process, so an edit written straight to disk stays
/// unreconciled, which is exactly the state one test needs.
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

async fn layout(
    client: &mut Client,
    req: pb::WorkspaceLayoutRequest,
) -> Result<pb::WorkspaceLayoutInfo, tonic::Status> {
    client
        .workspace_layout(req)
        .await
        .map(tonic::Response::into_inner)
}

fn set(refs_dir: &str, move_dirs: bool) -> pb::WorkspaceLayoutRequest {
    pb::WorkspaceLayoutRequest {
        set: true,
        refs_dir: refs_dir.into(),
        move_dirs,
        ..pb::WorkspaceLayoutRequest::default()
    }
}

async fn ref_dir(client: &mut Client, ensure: bool) -> pb::RefDirInfo {
    client
        .ref_dir(pb::RefDirRequest {
            path: "todo.txt".into(),
            task: Some(pb::TaskRef {
                line_number: 1,
                task_id: String::new(),
            }),
            ensure,
            workspace: None,
        })
        .await
        .unwrap()
        .into_inner()
}

async fn served(todo: &str) -> (tempfile::TempDir, Client, tokio::sync::oneshot::Sender<()>) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("todo.txt"), todo).unwrap();
    let (client, stop) = serve_in(dir.path(), IdentityMode::Sidecar).await;
    (dir, client, stop)
}

#[tokio::test]
async fn reading_a_workspace_with_no_file_gives_the_defaults() {
    let (_dir, mut client, _stop) = served("plan the launch\n").await;
    let info = layout(&mut client, pb::WorkspaceLayoutRequest::default())
        .await
        .unwrap();
    assert_eq!(
        (info.refs_dir.as_str(), info.todo_file.as_str()),
        ("tasks", "todo.txt")
    );
    assert_eq!((info.note.as_str(), info.moved), ("", 0));
}

#[tokio::test]
async fn setting_the_layout_with_nothing_in_the_way_writes_the_file_and_takes_effect() {
    let (dir, mut client, _stop) = served("plan the launch\n").await;
    let info = layout(&mut client, set("stuff", false)).await.unwrap();
    assert_eq!(info.refs_dir, "stuff");
    let toml = std::fs::read_to_string(dir.path().join("txtodo.toml")).unwrap();
    assert!(toml.contains("refs_dir = \"stuff\""), "{toml}");
    assert!(ref_dir(&mut client, false).await.dir.starts_with("stuff/"));
}

#[tokio::test]
async fn a_change_is_refused_while_a_ref_dir_exists_and_moves_them_when_asked() {
    let (dir, mut client, _stop) = served("plan the launch\n").await;
    let made = ref_dir(&mut client, true).await;
    assert!(made.dir.starts_with("tasks/"), "{made:?}");
    std::fs::write(dir.path().join(&made.dir).join("notes.md"), "keep me\n").unwrap();

    let refused = layout(&mut client, set("stuff", false)).await.unwrap_err();
    assert_eq!(refused.code(), Code::FailedPrecondition, "{refused:?}");
    assert!(dir.path().join(&made.dir).is_dir(), "nothing moved");
    assert!(
        !dir.path().join("txtodo.toml").exists(),
        "and no file was written"
    );
    assert_eq!(
        layout(&mut client, pb::WorkspaceLayoutRequest::default())
            .await
            .unwrap()
            .refs_dir,
        "tasks"
    );

    let done = layout(&mut client, set("stuff", true)).await.unwrap();
    assert_eq!((done.refs_dir.as_str(), done.moved), ("stuff", 1));
    let slug = made.dir.trim_start_matches("tasks/");
    assert!(!dir.path().join("tasks").join(slug).exists());
    assert_eq!(
        std::fs::read_to_string(dir.path().join("stuff").join(slug).join("notes.md")).unwrap(),
        "keep me\n"
    );
    assert!(ref_dir(&mut client, false).await.dir.starts_with("stuff/"));
}

#[tokio::test]
async fn an_unsafe_path_or_another_todo_file_is_refused_and_changes_nothing() {
    let (dir, mut client, _stop) = served("plan the launch\n").await;
    for bad in ["../up", "/abs", ".txtodo/x", "C:/x"] {
        let e = layout(&mut client, set(bad, false)).await.unwrap_err();
        assert_eq!(e.code(), Code::InvalidArgument, "{bad}: {e:?}");
    }
    let e = layout(
        &mut client,
        pb::WorkspaceLayoutRequest {
            set: true,
            todo_file: "work.txt".into(),
            ..Default::default()
        },
    )
    .await
    .unwrap_err();
    assert_eq!(e.code(), Code::InvalidArgument);
    assert!(!dir.path().join("txtodo.toml").exists());
}

#[tokio::test]
async fn a_folder_outside_refs_dir_that_nothing_points_at_is_reported() {
    let (dir, mut client, _stop) = served("plan the launch\n").await;
    std::fs::create_dir_all(dir.path().join("docs")).unwrap();
    std::fs::write(dir.path().join("docs/notes.md"), "old\n").unwrap();
    let info = layout(&mut client, pb::WorkspaceLayoutRequest::default())
        .await
        .unwrap();
    assert_eq!(info.outside_refs_dir, vec!["docs".to_owned()]);
}
