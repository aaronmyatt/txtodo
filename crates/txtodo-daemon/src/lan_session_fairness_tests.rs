//! Workspaces take turns on one link (task `sync-link-fairness`, 2026-09-25): `Live::tick` sends
//! at most one batch per workspace per turn, and none past `WINDOW_BATCHES` unacked, so a small
//! change in one workspace goes out on the next turn even while another moves thousands of ops.
//! Driven by hand over a link that only records what was sent, so the order is exact.

use std::collections::BTreeMap;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use txtodo_model::IdentityMode;
use txtodo_store::WorkspaceId;
use txtodo_sync::{
    Frame, GroupKey, Heads, Link, LinkError, Message, OriginRange, derive_group_op_signing_key,
    peek_workspace, want,
};

use crate::device_relay::WorkspaceRoute;
use crate::lan_session::read_heads;
use crate::lan_session_live::{Live, PushCtx};
use crate::server::SharedWorkspace;

/// Keeps every frame sent; never has one to receive.
#[derive(Default)]
struct Recorder {
    sent: Vec<Frame>,
}

impl Link for Recorder {
    fn send(&mut self, frame: Frame) -> Result<(), LinkError> {
        self.sent.push(frame);
        Ok(())
    }

    fn recv(&mut self) -> Result<Frame, LinkError> {
        Err(LinkError::Closed)
    }

    fn recv_timeout(&mut self, _wait: Duration) -> Result<Option<Frame>, LinkError> {
        Ok(None)
    }
}

impl Recorder {
    /// The workspace of each frame sent since the last call.
    fn drain(&mut self) -> Vec<WorkspaceId> {
        self.sent
            .drain(..)
            .filter_map(|f| peek_workspace(&f.body))
            .collect()
    }
}

/// A workspace whose `todo.txt` starts with `lines` tasks: adopting it logs one op per line.
fn workspace(dir: &std::path::Path, lines: usize, start_ms: u64) -> SharedWorkspace {
    let text: String = (0..lines).map(|i| format!("task {i}\n")).collect();
    std::fs::write(dir.join("todo.txt"), text).unwrap_or_else(|e| panic!("write: {e}"));
    let clock: Arc<dyn crate::clock::Clock> = Arc::new(crate::clock::FakeClock::new(start_ms));
    let ws = crate::workspace::Workspace::open_with_default_mode(dir, clock, IdentityMode::Tagged)
        .unwrap_or_else(|e| panic!("open workspace: {e}"));
    Arc::new(RwLock::new(ws))
}

fn want_all(ws: &SharedWorkspace, id: WorkspaceId) -> Message {
    Message::Want {
        workspace: id.ulid().to_u128(),
        ranges: want(&Heads::new(), &read_heads(ws)),
    }
}

fn greet_empty(id: WorkspaceId) -> Message {
    Message::Greet {
        workspace: id.ulid().to_u128(),
        heads: Heads::new(),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_big_transfer_and_a_small_one_take_turns_and_the_window_holds() {
    let (d1, d2) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let (big, small) = (
        workspace(d1.path(), 2_500, 1_000),
        workspace(d2.path(), 1, 9_000),
    );
    let id = |ws: &SharedWorkspace| ws.read().unwrap().workspace_id();
    let (big_id, small_id) = (id(&big), id(&small));
    let group = big.read().unwrap().group();
    let key = GroupKey::from_bytes([7u8; 32]);
    let signing_key = derive_group_op_signing_key(&key);
    let mut routes = BTreeMap::new();
    for ws in [&big, &small] {
        let device = ws.read().unwrap().device();
        let route = WorkspaceRoute {
            ws: Arc::clone(ws),
            device,
            group,
        };
        routes.insert(id(ws), route);
    }
    let ctx = PushCtx {
        group,
        key: &key,
        signing_key: &signing_key,
        routes: &routes,
    };
    let mut live = Live::new();
    for (ws, ws_id) in [(&big, big_id), (&small, small_id)] {
        live.observe(ws_id, &greet_empty(ws_id));
        live.observe(ws_id, &want_all(ws, ws_id));
    }
    let mut link = Recorder::default();

    assert!(live.tick(&mut link, &ctx));
    let mut first = link.drain();
    first.sort();
    let mut both = vec![big_id, small_id];
    both.sort();
    assert_eq!(
        first, both,
        "one batch each, not the big one's whole backlog first"
    );
    assert!(live.tick(&mut link, &ctx));
    assert_eq!(link.drain(), vec![big_id], "the big one's second batch");
    assert!(live.tick(&mut link, &ctx));
    assert_eq!(
        link.drain(),
        Vec::new(),
        "two batches unacked: the window is full"
    );

    let device = *read_heads(&big)
        .keys()
        .next()
        .unwrap_or_else(|| panic!("no ops"));
    let ack = Message::Ack {
        workspace: big_id.ulid().to_u128(),
        committed: vec![OriginRange {
            device,
            first: 1,
            last: 1_000,
        }],
    };
    live.observe(big_id, &ack);
    assert!(live.tick(&mut link, &ctx));
    assert_eq!(link.drain(), vec![big_id], "an ack opens the window again");
}
