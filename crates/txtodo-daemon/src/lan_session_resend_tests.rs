//! A run counts as delivered only once the peer acks it (task `sync-ack-before-held`,
//! 2026-09-25). Two real workspaces over one `ChannelLink` pair, each end the real
//! `drive_shared_session`; a wrapper drops frames on one side for a while to stand in for a batch
//! the peer never took, or an ack still on its way. `RESEND_AFTER` is 300 ms under test.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use tokio::task::JoinHandle;
use txtodo_model::{DeviceId, FilePath, Hlc, Op, OpId, OpKind, Principal, TaskId, Ulid};
use txtodo_sync::{ChannelLink, Frame, Link, LinkError, OriginRange, channel_link_pair};

use crate::lan_apply::{commit_incoming_ops, landed_ranges};
use crate::lan_session_push_tests::{add_line, text_of, wait_for_line, workspace_with_list};
use crate::lan_session_tests::{drive_session, make_workspace, peer_device};
use crate::server::SharedWorkspace;

/// A link that drops outbound frames while `drop_out` is set (only frames longer than
/// `min_len`, so a test can drop an `Ops` batch and keep the small heartbeat `Ack`s), and ends
/// once `stop` is set.
struct LossyLink {
    inner: ChannelLink,
    stop: Arc<AtomicBool>,
    drop_out: Arc<AtomicBool>,
    min_len: usize,
    dropped: Arc<AtomicUsize>,
}

impl Link for LossyLink {
    fn send(&mut self, frame: Frame) -> Result<(), LinkError> {
        if self.drop_out.load(Ordering::Relaxed) && frame.body.len() > self.min_len {
            self.dropped.fetch_add(1, Ordering::Relaxed);
            return Ok(());
        }
        self.inner.send(frame)
    }

    fn recv(&mut self) -> Result<Frame, LinkError> {
        self.inner.recv()
    }

    fn recv_timeout(&mut self, wait: Duration) -> Result<Option<Frame>, LinkError> {
        if self.stop.load(Ordering::Relaxed) {
            return Err(LinkError::Closed);
        }
        self.inner.recv_timeout(wait)
    }
}

/// Ends both drivers when dropped, so a failed assertion does not hang the runtime's shutdown.
struct StopOnDrop(Arc<AtomicBool>);

impl Drop for StopOnDrop {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Relaxed);
    }
}

/// One side's loss controls.
struct Loss {
    drop_out: Arc<AtomicBool>,
    dropped: Arc<AtomicUsize>,
}

/// Two linked workspaces, their drivers, and each side's loss controls.
struct Pair {
    a: SharedWorkspace,
    b: SharedWorkspace,
    device_b: DeviceId,
    loss_a: Loss,
    loss_b: Loss,
    stop: Arc<AtomicBool>,
    drivers: Vec<JoinHandle<bool>>,
    _dirs: (tempfile::TempDir, tempfile::TempDir),
}

/// Frames at or under this many bytes are never dropped: heartbeats and link handshakes.
const OPS_FRAME_MIN: usize = 200;

fn start_pair(min_len_b: usize) -> Pair {
    let dirs = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let key = [7u8; 32];
    let a = workspace_with_list(dirs.0.path(), key, 1_000);
    let b = workspace_with_list(dirs.1.path(), key, 2_000);
    let (group, device_a, device_b) = {
        let (ra, rb) = (a.read().unwrap(), b.read().unwrap());
        rb.set_group(ra.group());
        rb.set_workspace_id(ra.workspace_id());
        (ra.group(), ra.device(), rb.device())
    };
    let stop = Arc::new(AtomicBool::new(false));
    let loss = || Loss {
        drop_out: Arc::new(AtomicBool::new(false)),
        dropped: Arc::new(AtomicUsize::new(0)),
    };
    let (loss_a, loss_b) = (loss(), loss());
    let (link_a, link_b) = channel_link_pair();
    let mut drivers = Vec::new();
    for (inner, ws, device, l, min_len) in [
        (link_a, Arc::clone(&a), device_a, &loss_a, OPS_FRAME_MIN),
        (link_b, Arc::clone(&b), device_b, &loss_b, min_len_b),
    ] {
        let mut link = LossyLink {
            inner,
            stop: Arc::clone(&stop),
            drop_out: Arc::clone(&l.drop_out),
            min_len,
            dropped: Arc::clone(&l.dropped),
        };
        drivers.push(tokio::task::spawn_blocking(move || {
            drive_session(&mut link, ws, device, group)
        }));
    }
    Pair {
        a,
        b,
        device_b,
        loss_a,
        loss_b,
        stop,
        drivers,
        _dirs: dirs,
    }
}

async fn wait_until(what: &str, mut ok: impl FnMut() -> bool) {
    let started = Instant::now();
    while !ok() {
        assert!(started.elapsed() < Duration::from_secs(20), "{what}");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

async fn finish(pair: Pair) {
    let (a, device_b) = (Arc::clone(&pair.a), pair.device_b);
    assert!(
        a.read().unwrap().live_peers().is_live(device_b),
        "the session is still up"
    );
    pair.stop.store(true, Ordering::Relaxed);
    for driver in pair.drivers {
        assert!(driver.await.unwrap(), "both sessions greeted");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_batch_the_peer_never_got_is_sent_again_in_the_same_session() {
    let pair = start_pair(OPS_FRAME_MIN);
    let _stop = StopOnDrop(Arc::clone(&pair.stop));
    let (a, device_b) = (Arc::clone(&pair.a), pair.device_b);
    wait_until("never linked", || {
        a.read().unwrap().live_peers().is_live(device_b)
    })
    .await;

    pair.loss_a.drop_out.store(true, Ordering::Relaxed);
    add_line(&pair.a, "lost once").await;
    let dropped = Arc::clone(&pair.loss_a.dropped);
    wait_until("the push was never sent", || {
        dropped.load(Ordering::Relaxed) > 0
    })
    .await;
    pair.loss_a.drop_out.store(false, Ordering::Relaxed);

    wait_for_line(&pair.b, "lost once").await;
    add_line(&pair.a, "after it").await;
    wait_for_line(&pair.b, "after it").await;
    let b_text = text_of(&pair.b).unwrap_or_default();
    assert_eq!(b_text.matches("lost once").count(), 1, "{b_text}");
    finish(pair).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_copy_of_a_batch_already_held_is_acked_and_the_session_carries_on() {
    // Every frame B sends is dropped for a while, its Ack for the batch included.
    let pair = start_pair(0);
    let _stop = StopOnDrop(Arc::clone(&pair.stop));
    let (a, device_b) = (Arc::clone(&pair.a), pair.device_b);
    wait_until("never linked", || {
        a.read().unwrap().live_peers().is_live(device_b)
    })
    .await;

    pair.loss_b.drop_out.store(true, Ordering::Relaxed);
    add_line(&pair.a, "acked late").await;
    wait_for_line(&pair.b, "acked late").await;
    let dropped = Arc::clone(&pair.loss_b.dropped);
    wait_until("B never acked", || dropped.load(Ordering::Relaxed) > 0).await;
    pair.loss_b.drop_out.store(false, Ordering::Relaxed);

    // A resends after RESEND_AFTER; B skips the copy and says what it holds.
    tokio::time::sleep(Duration::from_millis(900)).await;
    add_line(&pair.a, "still linked").await;
    wait_for_line(&pair.b, "still linked").await;
    let b_text = text_of(&pair.b).unwrap_or_default();
    assert_eq!(b_text.matches("acked late").count(), 1, "{b_text}");
    finish(pair).await;
}

fn peer_insert(n: u128, file: &str) -> Op {
    let task = TaskId::new(Ulid::from_u128(1_000 + n));
    Op {
        id: OpId::new(Ulid::from_u128(5_000 + n)),
        hlc: Hlc {
            wall_ms: 1_000 + u64::try_from(n).unwrap_or(0),
            counter: 0,
            device: peer_device(),
        },
        principal: Principal::User {
            device: peer_device(),
        },
        file: FilePath::new(file).unwrap_or_else(|e| panic!("{e}")),
        kind: OpKind::Insert {
            task,
            after: None,
            line: format!("task {n} id:{task}"),
        },
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_failed_run_stops_the_batch_so_the_log_never_holds_a_hole() {
    let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
    let (ws, ..) = make_workspace(dir.path(), [7u8; 32]);
    // A plain file where a ref directory would go: `blocked/todo.txt` cannot be created.
    std::fs::write(dir.path().join("blocked"), "").unwrap_or_else(|e| panic!("write: {e}"));
    let ops = vec![
        peer_insert(1, "todo.txt"),
        peer_insert(2, "todo.txt"),
        peer_insert(3, "blocked/todo.txt"),
        peer_insert(4, "todo.txt"),
    ];
    let rt = tokio::runtime::Handle::current();
    let landed = {
        let ws = Arc::clone(&ws);
        tokio::task::spawn_blocking(move || commit_incoming_ops(&ws, &rt, ops))
            .await
            .unwrap_or_else(|e| panic!("join: {e}"))
    };
    assert_eq!(landed, 2, "the run before the failure, nothing after it");
    let held = crate::lan_session::read_heads(&ws);
    assert_eq!(held.get(&peer_device()), Some(&2));
    let text = text_of(&ws).unwrap_or_default();
    assert!(
        text.contains("task 2") && !text.contains("task 4"),
        "{text}"
    );
}

#[test]
fn landed_ranges_ack_only_the_ops_that_landed() {
    let r = |device: u128, first: u64, last: u64| OriginRange {
        device: DeviceId::new(Ulid::from_u128(device)),
        first,
        last,
    };
    let ranges = [r(1, 5, 9), r(2, 1, 3)];
    assert_eq!(landed_ranges(&ranges, 0), Vec::new());
    assert_eq!(landed_ranges(&ranges, 3), vec![r(1, 5, 7)]);
    assert_eq!(landed_ranges(&ranges, 5), vec![r(1, 5, 9)]);
    assert_eq!(landed_ranges(&ranges, 6), vec![r(1, 5, 9), r(2, 1, 1)]);
    assert_eq!(landed_ranges(&ranges, 99), ranges.to_vec());
}

#[tokio::test(flavor = "multi_thread")]
async fn ops_on_a_worktree_copy_land_in_the_log_but_never_on_disk() {
    let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
    let (ws, ..) = make_workspace(dir.path(), [7u8; 32]);
    let ops = vec![
        peer_insert(1, ".claude/worktrees/wt/todo.txt"),
        peer_insert(2, ".claude/worktrees/wt/todo.txt"),
        peer_insert(3, "todo.txt"),
    ];
    let rt = tokio::runtime::Handle::current();
    let landed = {
        let ws = Arc::clone(&ws);
        tokio::task::spawn_blocking(move || commit_incoming_ops(&ws, &rt, ops))
            .await
            .unwrap_or_else(|e| panic!("join: {e}"))
    };
    assert_eq!(landed, 3);
    let held = crate::lan_session::read_heads(&ws);
    assert_eq!(held.get(&peer_device()), Some(&3), "heads stay dense");
    assert!(
        !dir.path().join(".claude").exists(),
        "no worktree copy written"
    );
    assert!(text_of(&ws).unwrap_or_default().contains("task 3"));
}
