//! `Mutation::Replace` over the socket, in-process: a whole-document compare-and-swap. Refuses a
//! stale base hash (another `Apply` landed since the caller read) and an unreconciled edit on disk,
//! changing nothing; otherwise reconciles the new bytes like an external edit (untouched lines keep
//! their identity) but attributed to the caller. This is what the CLI's fallback sends instead of
//! writing the file itself, which used to drop a concurrent `Apply` silently. `RequireBase` is the
//! same check as a leading precondition on an ordinary batch: sidecar text has no `id:` to catch a
//! shifted line, so a line-number-only batch names the hash it was built from instead.
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

fn apply_req(kinds: Vec<mutation::Kind>) -> pb::ApplyRequest {
    pb::ApplyRequest {
        path: "todo.txt".into(),
        mutations: kinds
            .into_iter()
            .map(|kind| pb::Mutation { kind: Some(kind) })
            .collect(),
        agent: None,
        workspace: None,
        ..pb::ApplyRequest::default()
    }
}

fn replace(base_hash: &[u8], contents: &str) -> mutation::Kind {
    mutation::Kind::Replace(pb::Replace {
        base_hash: base_hash.to_vec(),
        contents: contents.as_bytes().to_vec(),
    })
}

fn require_base(base_hash: &[u8]) -> mutation::Kind {
    mutation::Kind::RequireBase(pb::RequireBase {
        base_hash: base_hash.to_vec(),
    })
}

/// A line addressed by number alone, the only way sidecar text can be addressed.
fn line_ref(line_number: u32) -> Option<pb::TaskRef> {
    Some(pb::TaskRef {
        line_number,
        task_id: String::new(),
    })
}

fn edit_line(line_number: u32, new_line: &str) -> mutation::Kind {
    mutation::Kind::Edit(pb::Edit {
        task: line_ref(line_number),
        new_line: new_line.into(),
    })
}

fn add(line: &str) -> mutation::Kind {
    mutation::Kind::Add(pb::Add { line: line.into() })
}

/// The document as a client reads it: text and hash.
async fn read(client: &mut Client) -> (String, Vec<u8>) {
    let file = client
        .get_file(pb::GetFileRequest {
            path: "todo.txt".into(),
            workspace: None,
        })
        .await
        .unwrap()
        .into_inner();
    (String::from_utf8(file.bytes).unwrap(), file.hash)
}

fn id_of(line: &str) -> &str {
    line.rsplit_once(" id:").expect("id tag").1
}

#[tokio::test]
async fn a_current_base_replaces_and_untouched_lines_keep_their_ids() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("todo.txt"), "one\ntwo\nthree\n").unwrap();
    let (mut client, _stop) = serve_in(dir.path(), IdentityMode::Tagged).await;
    let (text, hash) = read(&mut client).await;
    let lines: Vec<&str> = text.lines().collect();

    // Drop `two`, tag `three`, append `four` with no id yet.
    let edited = format!(
        "{}\n{}\nfour\n",
        lines[0],
        lines[2].replacen("three", "three @home", 1)
    );
    let reply = client
        .apply(apply_req(vec![replace(&hash, &edited)]))
        .await
        .unwrap()
        .into_inner();
    assert!(reply.applied > 0, "ops were appended");

    let (after, after_hash) = read(&mut client).await;
    let now: Vec<&str> = after.lines().collect();
    assert_eq!(now.len(), 3, "{after}");
    assert_eq!(now[0], lines[0], "an untouched line is byte-identical");
    assert!(now[1].starts_with("three @home id:"), "{after}");
    assert_eq!(
        id_of(now[1]),
        id_of(lines[2]),
        "the edited line kept its id"
    );
    assert!(
        now[2].starts_with("four id:"),
        "the new line got an id: {after}"
    );
    assert_eq!(reply.hash, after_hash, "the reply names the new hash");
    assert_eq!(
        std::fs::read_to_string(dir.path().join("todo.txt")).unwrap(),
        after
    );

    let history = client
        .history(pb::HistoryRequest {
            path: "todo.txt".into(),
            task_id: String::new(),
            limit: 1,
            before_seq: 0,
            workspace: None,
        })
        .await
        .unwrap()
        .into_inner()
        .ops;
    assert!(history[0].principal.starts_with("you@"), "{history:?}");
}

#[tokio::test]
async fn a_stale_base_is_refused_and_nothing_changes() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("todo.txt"), "one\ntwo\n").unwrap();
    let (mut a, _stop) = serve_in(dir.path(), IdentityMode::Tagged).await;
    let (snapshot, stale_hash) = read(&mut a).await;
    let mut b = connect(dir.path().join(".txtodo").join("txtodod.sock")).await;

    // Agent B commits an add after agent A read the file.
    b.apply(apply_req(vec![add("from B")])).await.unwrap();
    let (with_b, fresh_hash) = read(&mut b).await;

    // Agent A's whole-file write, based on what it read, is refused.
    let err = a
        .apply(apply_req(vec![replace(
            &stale_hash,
            &snapshot.replace("one", "ONE"),
        )]))
        .await
        .unwrap_err();
    assert_eq!(err.code(), Code::FailedPrecondition, "{err}");
    assert_eq!(
        read(&mut a).await,
        (with_b.clone(), fresh_hash.clone()),
        "untouched"
    );

    // Re-read and retry: accepted, and B's line survives.
    a.apply(apply_req(vec![replace(
        &fresh_hash,
        &with_b.replace("one", "ONE"),
    )]))
    .await
    .unwrap();
    let (retried, _) = read(&mut a).await;
    assert!(
        retried.starts_with("ONE id:") && retried.contains("from B id:"),
        "{retried}"
    );
}

#[tokio::test]
async fn an_unreconciled_edit_on_disk_is_not_overwritten() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("todo.txt"), "one\ntwo\n").unwrap();
    let (mut client, _stop) = serve_in(dir.path(), IdentityMode::Tagged).await;
    let (text, hash) = read(&mut client).await;

    // An editor save the daemon has not reconciled: its projection, and so `hash`, is unchanged.
    std::fs::write(dir.path().join("todo.txt"), "typed in vim\n").unwrap();
    let err = client
        .apply(apply_req(vec![replace(&hash, &text.replace("one", "ONE"))]))
        .await
        .unwrap_err();
    assert_eq!(err.code(), Code::FailedPrecondition, "{err}");
    let disk = std::fs::read_to_string(dir.path().join("todo.txt")).unwrap();
    assert_eq!(disk, "typed in vim\n", "the editor's bytes were left alone");
}

#[tokio::test]
async fn bad_requests_are_refused_and_a_no_op_replace_appends_nothing() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("todo.txt"), "one\n").unwrap();
    let (mut client, _stop) = serve_in(dir.path(), IdentityMode::Tagged).await;
    let (text, hash) = read(&mut client).await;

    let short = client
        .apply(apply_req(vec![replace(&hash[..5], &text)]))
        .await;
    assert_eq!(
        short.unwrap_err().code(),
        Code::InvalidArgument,
        "base_hash is 32 bytes"
    );
    let mixed = apply_req(vec![add("two"), replace(&hash, &text)]);
    assert_eq!(
        client.apply(mixed).await.unwrap_err().code(),
        Code::InvalidArgument,
        "Replace must be alone in its batch"
    );
    assert_eq!(
        read(&mut client).await,
        (text.clone(), hash.clone()),
        "neither changed anything"
    );

    let same = client.apply(apply_req(vec![replace(&hash, &text)])).await;
    let same = same.unwrap().into_inner();
    assert_eq!(
        (same.applied, same.hash),
        (0, hash),
        "identical bytes: no ops, same hash"
    );
}

/// Sidecar mode has no `id:` tags, so nothing but the base hash can tell the caller its lines moved.
#[tokio::test]
async fn sidecar_mode_replaces_too_and_drops_a_blank_line() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("todo.txt"), "a\n\nb\n").unwrap();
    let (mut client, _stop) = serve_in(dir.path(), IdentityMode::Sidecar).await;
    let (text, hash) = read(&mut client).await;
    assert_eq!(text, "a\n\nb\n", "sidecar text carries no id tags");

    // What `archive` does: drops the blank. No other mutation can say that.
    client
        .apply(apply_req(vec![replace(&hash, "a\nb\n")]))
        .await
        .unwrap();
    assert_eq!(read(&mut client).await.0, "a\nb\n");

    let err = client
        .apply(apply_req(vec![replace(&hash, "z\n")]))
        .await
        .unwrap_err();
    assert_eq!(
        err.code(),
        Code::FailedPrecondition,
        "the old hash is stale now: {err}"
    );
}

/// What the desktop editor sends when a line is moved (task desktop-reorder-propagates): the same
/// lines in a new order, plus one edited in the same save. Each line keeps its task id, so its
/// history and notes follow it; sidecar text has no `id:` to show that, `GetFile`'s ids do.
#[tokio::test]
async fn sidecar_replace_with_moved_lines_keeps_each_lines_identity() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("todo.txt"), "one\ntwo\nthree\n").unwrap();
    let (mut client, _stop) = serve_in(dir.path(), IdentityMode::Sidecar).await;
    let get = pb::GetFileRequest {
        path: "todo.txt".into(),
        workspace: None,
    };
    let before = client.get_file(get.clone()).await.unwrap().into_inner();
    let ids = before.task_ids.clone();
    assert_eq!(ids.len(), 3);

    // `three` moves to the top, and `two` is edited on the way.
    client
        .apply(apply_req(vec![replace(
            &before.hash,
            "three\none\ntwo +p\n",
        )]))
        .await
        .unwrap();
    let after = client.get_file(get).await.unwrap().into_inner();
    assert_eq!(after.bytes, b"three\none\ntwo +p\n");
    assert_eq!(
        std::fs::read(dir.path().join("todo.txt")).unwrap(),
        after.bytes
    );
    let moved = vec![ids[2].clone(), ids[0].clone(), ids[1].clone()];
    assert_eq!(after.task_ids, moved, "ids follow their lines");
}

/// Agent A read `a b c` and means to edit `b` (line 2). Agent B then deletes line 1, so line 2 is
/// `c`: with no id to disagree, only the base hash can refuse A's edit.
#[tokio::test]
async fn a_guarded_line_number_batch_is_refused_once_the_lines_have_shifted() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("todo.txt"), "a\nb\nc\n").unwrap();
    let (mut a, _stop) = serve_in(dir.path(), IdentityMode::Sidecar).await;
    let (_, stale_hash) = read(&mut a).await;
    let mut b = connect(dir.path().join(".txtodo").join("txtodod.sock")).await;
    let delete = mutation::Kind::Delete(pb::Delete {
        task: line_ref(1),
        leave_blank: false,
    });
    b.apply(apply_req(vec![delete])).await.unwrap();

    let stale = apply_req(vec![require_base(&stale_hash), edit_line(2, "b edited")]);
    let err = a.apply(stale).await.unwrap_err();
    assert_eq!(err.code(), Code::FailedPrecondition, "{err}");
    let (after, fresh_hash) = read(&mut a).await;
    assert_eq!(after, "b\nc\n", "nothing was edited, `c` least of all");

    // Re-read, find `b` at line 1 now, retry with the fresh hash.
    let retry = apply_req(vec![require_base(&fresh_hash), edit_line(1, "b edited")]);
    a.apply(retry).await.unwrap();
    assert_eq!(read(&mut a).await.0, "b edited\nc\n");
}

#[tokio::test]
async fn require_base_must_lead_alone_and_not_hide_an_unreconciled_edit() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("todo.txt"), "a\n").unwrap();
    let (mut client, _stop) = serve_in(dir.path(), IdentityMode::Sidecar).await;
    let (_, hash) = read(&mut client).await;

    let not_first = apply_req(vec![add("b"), require_base(&hash)]);
    let twice = apply_req(vec![require_base(&hash), require_base(&hash)]);
    let short = apply_req(vec![require_base(&hash[..5])]);
    for bad in [not_first, twice, short] {
        let err = client.apply(bad).await.unwrap_err();
        assert_eq!(err.code(), Code::InvalidArgument, "{err}");
    }
    assert_eq!(
        read(&mut client).await,
        ("a\n".into(), hash.clone()),
        "nothing changed"
    );

    // The projection, and so `hash`, is current, but the file holds an editor save the daemon has
    // not reconciled: writing over it would lose it.
    std::fs::write(dir.path().join("todo.txt"), "typed in vim\n").unwrap();
    let guarded = apply_req(vec![require_base(&hash), add("b")]);
    let err = client.apply(guarded).await.unwrap_err();
    assert_eq!(err.code(), Code::FailedPrecondition, "{err}");
    let disk = std::fs::read_to_string(dir.path().join("todo.txt")).unwrap();
    assert_eq!(disk, "typed in vim\n");
}
