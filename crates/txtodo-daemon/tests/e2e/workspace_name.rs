//! Task workspace-vanity-name, two real daemons over LAN: B's mirror of A's workspace shows A's
//! name for it, not the id its folder is named by. First the folder name A offers; then, once A
//! renames the workspace, the new name, which reaches B's copy of `txtodo.toml`. A rename of B's
//! mirror travels back to A the same way.
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::print_stderr)]
#![cfg(unix)]

use crate::support;

use std::path::Path;
use std::time::{Duration, Instant};
use support::multi::{MultiClient, MultiWorkspaceDaemon, file_at};
use txtodo_proto::v1 as pb;

const DEADLINE: Duration = Duration::from_secs(90);

fn code(r: &pb::PairOfferResponse) -> String {
    serde_json::json!({
        "device": r.device,
        "group_id": r.group_id,
        "x25519_pub": r.x25519_pub,
        "endpoint": r.endpoint,
        "nonce": r.nonce,
        "identity_mode": r.identity_mode,
    })
    .to_string()
}

/// `a` offers, `b` joins, both say "my own device".
async fn pair(a: &mut MultiClient, b: &mut MultiClient) {
    let offer = a
        .pair_offer(pb::PairOfferRequest { workspace: None })
        .await
        .unwrap()
        .into_inner();
    let accept = pb::PairAcceptRequest {
        code: code(&offer),
        workspace: None,
    };
    let sas_b = b.pair_accept(accept).await.unwrap().into_inner().sas;
    let start = Instant::now();
    loop {
        let req = pb::PairAwaitPeerRequest { workspace: None };
        let sas_a = a.pair_await_peer(req).await.unwrap().into_inner().sas;
        if !sas_a.is_empty() {
            assert_eq!(sas_a, sas_b);
            break;
        }
        assert!(start.elapsed() < DEADLINE, "no joiner reached A");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    for client in [a, b] {
        let req = pb::PairConfirmRequest {
            workspace: None,
            own_device: true,
        };
        client.pair_confirm_sas(req).await.unwrap();
    }
}

/// `id` as `client` lists it, once it is listed at all.
async fn listed(client: &mut MultiClient, id: &str) -> Option<pb::WorkspaceInfo> {
    let req = pb::WorkspaceListRequest {};
    let all = client.workspace_list(req).await.unwrap().into_inner();
    all.workspaces.into_iter().find(|w| w.workspace_id == id)
}

/// Waits until `client` lists `id` under `name`; returns that entry.
async fn wait_named(
    client: &mut MultiClient,
    id: &str,
    name: &str,
    log: &dyn Fn() -> String,
) -> pb::WorkspaceInfo {
    let start = Instant::now();
    loop {
        let now = listed(client, id).await;
        if let Some(w) = now.as_ref().filter(|w| w.name == name) {
            return w.clone();
        }
        assert!(
            start.elapsed() < DEADLINE,
            "never listed as {name:?}; last: {now:?}\n{}",
            log()
        );
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

async fn rename(client: &mut MultiClient, id: &str, name: &str) -> pb::WorkspaceInfo {
    let req = pb::WorkspaceRenameRequest {
        workspace_id: id.to_owned(),
        name: name.to_owned(),
    };
    client.workspace_rename(req).await.unwrap().into_inner()
}

fn layout_file(root: &str) -> String {
    std::fs::read_to_string(Path::new(root).join("txtodo.toml")).unwrap_or_default()
}

/// Waits until `root`'s `txtodo.toml` holds `needle`; returns the file. A mirror can list a new
/// name before its file has it: the owner's offer carries the name over the control channel, and
/// the file follows as its own sync. CI once read the file between the two (run 36285146366).
async fn wait_file_has(root: &str, needle: &str, log: &dyn Fn() -> String) -> String {
    let start = Instant::now();
    loop {
        let text = layout_file(root);
        if text.contains(needle) {
            return text;
        }
        assert!(
            start.elapsed() < DEADLINE,
            "{needle:?} never reached {root}/txtodo.toml; last:\n{text}\n{}",
            log()
        );
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

/// A's workspace in a folder named `plants`, registered and open.
async fn plants(client: &mut MultiClient, work: &Path) -> pb::WorkspaceInfo {
    let root = work.join("plants");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("todo.txt"), "water the fern\n").unwrap();
    let req = pb::WorkspaceAddRequest {
        root: root.display().to_string(),
    };
    let w = client.workspace_add(req).await.unwrap().into_inner();
    file_at(client, Path::new(&w.root)).await;
    w
}

#[tokio::test]
async fn a_name_set_on_one_device_shows_on_the_other() {
    let (dir_a, dir_b, work) = (
        tempfile::tempdir().unwrap(),
        tempfile::tempdir().unwrap(),
        tempfile::tempdir().unwrap(),
    );
    let (a, mut client_a) = MultiWorkspaceDaemon::start_with_args(dir_a, &[]).await;
    let (b, mut client_b) = MultiWorkspaceDaemon::start_with_args(dir_b, &[]).await;
    let logs = || format!("--- A ---\n{}\n--- B ---\n{}", a.log_tail(), b.log_tail());
    let w = plants(&mut client_a, work.path()).await;
    assert_eq!(w.name, "plants", "A shows the folder's name");
    pair(&mut client_a, &mut client_b).await;

    // B's mirror sits in a folder named by the id, and shows what A offers it as.
    let mirror = wait_named(&mut client_b, &w.workspace_id, "plants", &logs).await;
    assert!(mirror.is_remote, "{mirror:?}");
    assert!(mirror.root.ends_with(&w.workspace_id), "{mirror:?}");

    let renamed = rename(&mut client_a, &w.workspace_id, "House plants").await;
    assert_eq!(renamed.name, "House plants");
    let mirror = wait_named(&mut client_b, &w.workspace_id, "House plants", &logs).await;
    let text = wait_file_has(&mirror.root, "name = \"House plants\"", &logs).await;
    assert_eq!(
        text.matches("name =").count(),
        1,
        "B's file has one name line:\n{text}"
    );

    // And back: B names its mirror, and A shows that name for its own folder.
    rename(&mut client_b, &w.workspace_id, "Plants \u{1f33f}").await;
    wait_named(&mut client_a, &w.workspace_id, "Plants \u{1f33f}", &logs).await;
    let text = layout_file(&w.root);
    assert_eq!(text.matches("name =").count(), 1, "one name line:\n{text}");
}
