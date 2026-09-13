//! `DebugSetGroupKey` end to end over the real socket (plan M4 `sync-lan-transport`): refused
//! without `TXTODO_TEST_HOOKS=1`, and once set, updates `Health`'s `lan_group_key_present` and
//! the group id `Health` would otherwise never expose directly. One test, not two: the env var is
//! process-global, so both states run sequentially in the same test rather than racing a sibling
//! test in this binary.
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
use txtodo_proto::v1::{self as pb};

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
    let ws = Workspace::open_with_default_mode(root, Arc::new(SystemClock), IdentityMode::Sidecar)
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

#[tokio::test]
async fn refused_without_the_env_var_then_works_once_set() {
    // SAFETY: this test binary has no `#![forbid(unsafe_code)]` (that lives in the library
    // crate's own root, a separate compilation) and mutates its own process's env only.
    unsafe {
        std::env::remove_var(txtodo_daemon::debug_hooks::TEST_HOOKS_ENV_VAR);
    }
    let dir = tempfile::tempdir().unwrap();
    let (mut client, _stop) = serve(dir.path()).await;

    let health_before = client
        .health(pb::HealthRequest {})
        .await
        .unwrap()
        .into_inner();
    assert!(!health_before.lan_group_key_present);

    let req = pb::DebugSetGroupKeyRequest {
        group_id: "42".to_string(),
        key_hex: "11".repeat(32),
    };
    let refused = client.debug_set_group_key(req.clone()).await;
    assert_eq!(
        refused.unwrap_err().code(),
        tonic::Code::Unimplemented,
        "disabled by default"
    );

    // SAFETY: see above.
    unsafe {
        std::env::set_var(txtodo_daemon::debug_hooks::TEST_HOOKS_ENV_VAR, "1");
    }
    client
        .debug_set_group_key(req)
        .await
        .unwrap_or_else(|e| panic!("debug_set_group_key: {e}"));
    // SAFETY: see above.
    unsafe {
        std::env::remove_var(txtodo_daemon::debug_hooks::TEST_HOOKS_ENV_VAR);
    }

    let health_after = client
        .health(pb::HealthRequest {})
        .await
        .unwrap()
        .into_inner();
    assert!(health_after.lan_group_key_present);
    assert!(health_after.lan_relay_disabled);
}
