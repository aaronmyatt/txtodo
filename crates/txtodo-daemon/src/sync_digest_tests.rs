//! ADR 0035 end to end over one live session: two real workspaces driven by the real
//! `drive_shared_session` over a `ChannelLink` pair. The test seam makes B report another byte
//! hash for todo.txt (`ActorMsg::SkewDigestForTest`); the next quiet period's digest books it as
//! split on A (same ops, different bytes), and once B's bytes agree again a later digest clears it.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, PoisonError};
use std::time::{Duration, Instant};

use txtodo_model::{DeviceId, FilePath};
use txtodo_sync::{ChannelLink, Frame, Link, LinkError, channel_link_pair};

use crate::handle::ActorMsg;
use crate::lan_session_push_tests::{add_line, wait_for_line, workspace_with_list};
use crate::lan_session_tests::drive_session;
use crate::server::SharedWorkspace;

/// A `ChannelLink` the test closes from outside (as `lan_session_push_tests.rs`'s).
struct StoppableLink {
    inner: ChannelLink,
    stop: Arc<AtomicBool>,
}

impl Link for StoppableLink {
    fn send(&mut self, frame: Frame) -> Result<(), LinkError> {
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

/// Stops both links when dropped, so a failed assertion ends the drivers.
struct StopOnDrop(Arc<AtomicBool>);

impl Drop for StopOnDrop {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Relaxed);
    }
}

async fn skew(ws: &SharedWorkspace, on: bool) {
    let handle = ws
        .read()
        .unwrap_or_else(PoisonError::into_inner)
        .actor(&FilePath::new("todo.txt").unwrap_or_else(|e| panic!("{e}")))
        .cloned()
        .unwrap_or_else(|| panic!("todo.txt has an actor"));
    handle
        .ask(|reply| ActorMsg::SkewDigestForTest { on, reply })
        .await
        .unwrap_or_else(|e| panic!("skew: {e}"));
}

fn splits_on(ws: &SharedWorkspace, peer: DeviceId) -> Vec<String> {
    ws.read()
        .unwrap_or_else(PoisonError::into_inner)
        .split_files()
        .of(peer)
        .into_iter()
        .map(|(_, file, _)| file.as_str().to_owned())
        .collect()
}

/// Polls until `ws`'s splits with `peer` are `want`, or fails after a generous bound.
async fn wait_for_splits(ws: &SharedWorkspace, peer: DeviceId, want: &[&str]) {
    let started = Instant::now();
    loop {
        if splits_on(ws, peer) == want {
            return;
        }
        assert!(
            started.elapsed() < Duration::from_secs(20),
            "splits never became {want:?}: {:?}",
            splits_on(ws, peer)
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_file_rendered_differently_is_flagged_after_a_quiet_period_and_cleared_when_it_agrees() {
    let (dir_a, dir_b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let key = [7u8; 32];
    let a = workspace_with_list(dir_a.path(), key, 1_000);
    let b = workspace_with_list(dir_b.path(), key, 2_000);
    let (group, device_a, device_b) = {
        let (ra, rb) = (a.read().unwrap(), b.read().unwrap());
        rb.set_group(ra.group());
        rb.set_workspace_id(ra.workspace_id());
        (ra.group(), ra.device(), rb.device())
    };
    let stop = Arc::new(AtomicBool::new(false));
    let _stop_on_panic = StopOnDrop(Arc::clone(&stop));
    let (link_a, link_b) = channel_link_pair();
    let mut drivers = Vec::new();
    for (link, ws, device) in [
        (link_a, Arc::clone(&a), device_a),
        (link_b, Arc::clone(&b), device_b),
    ] {
        let mut link = StoppableLink {
            inner: link,
            stop: Arc::clone(&stop),
        };
        drivers.push(tokio::task::spawn_blocking(move || {
            drive_session(&mut link, ws, device, group)
        }));
    }

    add_line(&a, "buy milk").await;
    wait_for_line(&b, "buy milk").await;
    assert!(splits_on(&a, device_b).is_empty(), "same ops, same bytes");

    // B renders todo.txt differently; a commit makes both sides digest again once quiet.
    skew(&b, true).await;
    add_line(&a, "walk the dog").await;
    wait_for_line(&b, "walk the dog").await;
    wait_for_splits(&a, device_b, &["todo.txt"]).await;
    wait_for_splits(&b, device_a, &["todo.txt"]).await;

    // B agrees again: the next digest clears it on both.
    skew(&b, false).await;
    add_line(&b, "call mum").await;
    wait_for_line(&a, "call mum").await;
    wait_for_splits(&a, device_b, &[]).await;
    wait_for_splits(&b, device_a, &[]).await;

    stop.store(true, Ordering::Relaxed);
    for driver in drivers {
        assert!(driver.await.unwrap(), "both sessions greeted");
    }
}
