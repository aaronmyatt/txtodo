//! Capability tokens over the socket (plan M6, design §6.2): create → list → revoke, an
//! unrecognized scope refused at create time, and the revoked secret failing `Store::verify_token`
//! directly — the enforcement primitive a future request-time auth path will call (not reachable
//! over gRPC yet; see `crates/txtodo-daemon/src/tokens.rs`).
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
use txtodo_proto::v1::txtodo_client::TxtodoClient;
use txtodo_proto::v1::{self as pb};
use txtodo_store::Store;

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

/// Serves `root` on a socket inside it; the server task ends when the returned sender drops.
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

fn create_req(name: &str, scopes: &[&str]) -> pb::TokenCreateRequest {
    pb::TokenCreateRequest {
        name: name.into(),
        scopes: scopes.iter().map(|s| (*s).to_owned()).collect(),
        expires: String::new(),
        workspace: None,
    }
}

#[tokio::test]
async fn create_list_and_revoke_round_trip_over_the_socket() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("todo.txt"), "(A) seed\n").unwrap();
    let (mut client, _stop) = serve(dir.path()).await;

    let created = client
        .token_create(create_req(
            "claude-code",
            &["read", "write:add", "project:+work"],
        ))
        .await
        .unwrap_or_else(|e| panic!("create: {e}"))
        .into_inner();
    assert!(!created.id.is_empty());
    assert!(
        !created.secret.is_empty(),
        "the plaintext is handed back once"
    );
    assert_eq!(created.scopes, vec!["read", "write:add", "project:+work"]);

    let listed = client
        .token_list(pb::TokenListRequest { workspace: None })
        .await
        .unwrap_or_else(|e| panic!("list: {e}"))
        .into_inner()
        .tokens;
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, created.id, "a human can find it by id");
    assert_eq!(
        listed[0].scopes, created.scopes,
        "a human can see the scopes a token carries"
    );
    assert!(
        listed[0].secret.is_empty(),
        "TokenList never returns the secret"
    );

    let revoked = client
        .token_revoke(pb::TokenRevokeRequest {
            id: created.id.clone(),
            workspace: None,
        })
        .await
        .unwrap_or_else(|e| panic!("revoke: {e}"))
        .into_inner();
    assert!(revoked.revoked);

    let after = client
        .token_list(pb::TokenListRequest { workspace: None })
        .await
        .unwrap()
        .into_inner()
        .tokens;
    assert!(after.is_empty(), "a revoked token disappears from the list");

    // `verify_token` is not wired to any RPC yet (plan M6's larger auth server does that); it is
    // exercised directly against the same store the daemon just wrote, over the same socket path.
    let store = Store::open(&dir.path().join(".txtodo").join("oplog.db")).unwrap();
    assert!(matches!(
        store.verify_token(&created.secret, 0),
        Err(txtodo_store::TokenError::Revoked)
    ));
}

#[tokio::test]
async fn a_workspace_restrictor_round_trips_including_a_set_and_the_explicit_all() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("todo.txt"), "(A) seed\n").unwrap();
    let (mut client, _stop) = serve(dir.path()).await;

    // One workspace (an id), a set (two ids, repeated), and the explicit "every workspace" — all
    // three shapes task `mcp-workspace-scoped-tokens` added to the design §6.2 grammar.
    let created = client
        .token_create(create_req(
            "claude-code",
            &[
                "read",
                "workspace:01ARZ3NDEKTSV4RRFFQ69G5FAV",
                "workspace:01ARZ3NDEKTSV4RRFFQ69G5FAW",
            ],
        ))
        .await
        .unwrap_or_else(|e| panic!("create with a workspace set: {e}"))
        .into_inner();
    assert_eq!(
        created.scopes,
        vec![
            "read",
            "workspace:01ARZ3NDEKTSV4RRFFQ69G5FAV",
            "workspace:01ARZ3NDEKTSV4RRFFQ69G5FAW",
        ]
    );

    let all = client
        .token_create(create_req("desktop", &["read", "workspace:*"]))
        .await
        .unwrap_or_else(|e| panic!("create with explicit all: {e}"))
        .into_inner();
    assert_eq!(all.scopes, vec!["read", "workspace:*"]);
}

#[tokio::test]
async fn a_bare_workspace_restrictor_is_refused_at_create_time() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("todo.txt"), "(A) seed\n").unwrap();
    let (mut client, _stop) = serve(dir.path()).await;

    let err = client
        .token_create(create_req("claude-code", &["read", "workspace:"]))
        .await
        .unwrap_err();
    assert_eq!(err.code(), tonic::Code::InvalidArgument);

    let listed = client
        .token_list(pb::TokenListRequest { workspace: None })
        .await
        .unwrap()
        .into_inner()
        .tokens;
    assert!(listed.is_empty(), "the rejected create stored nothing");
}

#[tokio::test]
async fn an_unrecognized_scope_is_refused_at_create_time() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("todo.txt"), "(A) seed\n").unwrap();
    let (mut client, _stop) = serve(dir.path()).await;

    let err = client
        .token_create(create_req("claude-code", &["read", "sudo"]))
        .await
        .unwrap_err();
    assert_eq!(err.code(), tonic::Code::InvalidArgument);

    let listed = client
        .token_list(pb::TokenListRequest { workspace: None })
        .await
        .unwrap()
        .into_inner()
        .tokens;
    assert!(listed.is_empty(), "the rejected create stored nothing");
}
