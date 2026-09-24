//! Push on commit over one long-lived session (task `sync-live-push`): two real workspaces, each
//! driven by the real `drive_shared_session` over one `ChannelLink` pair, stay connected after the
//! first exchange, and a commit on either side reaches the other with no redial. The links are
//! wrapped so the test can end both sessions when it is done.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, PoisonError};
use std::time::{Duration, Instant};

use crate::mutation::Mutation;
use txtodo_model::{FilePath, Principal};
use txtodo_sync::{ChannelLink, Frame, Link, LinkError, channel_link_pair};

use crate::lan_session_tests::drive_session;
use crate::server::SharedWorkspace;

/// A `ChannelLink` the test can close from outside: once `stop` is set, the next wait reports the
/// link closed, and the driver returns.
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

/// Stops both links when dropped, so a failed assertion ends the drivers instead of leaving the
/// runtime's shutdown waiting on them forever.
struct StopOnDrop(Arc<AtomicBool>);

impl Drop for StopOnDrop {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Relaxed);
    }
}

fn text_of(ws: &SharedWorkspace) -> Option<String> {
    let handle = ws
        .read()
        .unwrap_or_else(PoisonError::into_inner)
        .actor(&FilePath::new("todo.txt").unwrap_or_else(|e| panic!("{e}")))
        .cloned()?;
    let rt = tokio::runtime::Handle::current();
    let bytes = tokio::task::block_in_place(|| rt.block_on(handle.get()))
        .ok()?
        .bytes;
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

async fn add_line(ws: &SharedWorkspace, line: &str) {
    let (handle, device) = {
        let guard = ws.read().unwrap_or_else(PoisonError::into_inner);
        let handle = guard
            .actor(&FilePath::new("todo.txt").unwrap_or_else(|e| panic!("{e}")))
            .cloned()
            .unwrap_or_else(|| panic!("todo.txt has an actor"));
        (handle, guard.device())
    };
    let add = Mutation::Add {
        line: line.to_owned(),
    };
    handle
        .apply(vec![add], Principal::User { device })
        .await
        .unwrap_or_else(|e| panic!("apply: {e}"));
}

/// Waits for `ws`'s todo.txt to hold `needle`. Generous: this machine is slow in bursts.
async fn wait_for_line(ws: &SharedWorkspace, needle: &str) -> Duration {
    let started = Instant::now();
    loop {
        if text_of(ws).is_some_and(|t| t.contains(needle)) {
            return started.elapsed();
        }
        assert!(
            started.elapsed() < Duration::from_secs(20),
            "{needle} never arrived: {:?}",
            text_of(ws)
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// `lan_session_tests::make_workspace`, with an empty `todo.txt` and its own clock start: two
/// `FakeClock`s started alike mint the same op ids, which one store then refuses as a repeat.
fn workspace_with_list(dir: &std::path::Path, key: [u8; 32], start_ms: u64) -> SharedWorkspace {
    std::fs::write(dir.join("todo.txt"), "").unwrap_or_else(|e| panic!("write: {e}"));
    let clock: Arc<dyn crate::clock::Clock> = Arc::new(crate::clock::FakeClock::new(start_ms));
    let ws = crate::workspace::Workspace::open_with_default_mode(
        dir,
        clock,
        txtodo_model::IdentityMode::Tagged,
    )
    .unwrap_or_else(|e| panic!("open workspace: {e}"));
    ws.key_store()
        .put(
            txtodo_sync::KeyId::Group(0),
            &txtodo_sync::Secret::new(key.to_vec()),
        )
        .unwrap_or_else(|e| panic!("seed group key: {e}"));
    Arc::new(std::sync::RwLock::new(ws))
}

#[tokio::test(flavor = "multi_thread")]
async fn a_commit_on_either_side_is_pushed_over_the_open_session() {
    let (dir_a, dir_b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let key = [7u8; 32];
    let a = workspace_with_list(dir_a.path(), key, 1_000);
    let b = workspace_with_list(dir_b.path(), key, 2_000);
    // One device-set: the same group and the same workspace id on both ends.
    let (group, id, device_a, device_b) = {
        let (ra, rb) = (a.read().unwrap(), b.read().unwrap());
        rb.set_group(ra.group());
        rb.set_workspace_id(ra.workspace_id());
        (ra.group(), ra.workspace_id(), ra.device(), rb.device())
    };
    assert_eq!(b.read().unwrap().workspace_id(), id);

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

    // Both ends mark each other live once the link handshake is done.
    let started = Instant::now();
    while !a.read().unwrap().live_peers().is_live(device_b) {
        assert!(started.elapsed() < Duration::from_secs(20), "never linked");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    add_line(&a, "pushed from a").await;
    wait_for_line(&b, "pushed from a").await;
    add_line(&b, "pushed from b").await;
    wait_for_line(&a, "pushed from b").await;
    let a_text = text_of(&a).unwrap_or_default();
    assert_eq!(
        a_text.matches("pushed from a").count(),
        1,
        "nothing echoed back: {a_text}"
    );

    stop.store(true, Ordering::Relaxed);
    for driver in drivers {
        assert!(driver.await.unwrap(), "both sessions greeted");
    }
    assert!(
        !a.read().unwrap().live_peers().is_live(device_b),
        "an ended session is no longer live"
    );
}
