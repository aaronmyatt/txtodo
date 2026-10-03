//! Three devices, each linked to the other two at once (own devices carry each other's ops, ADR
//! 0029's amendment), all adding lines: every device ends with every op. Lab chaos 1072683562
//! (2026-10-03): a device took an origin's ops through one session, relayed them through the
//! other, and that peer then pushed the origin's next op, which the first session refused as a gap
//! (its heads still stood where it opened). The sender rewound and sent it again, forever.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use crate::first_sync_bench_tests::device;
use crate::lan_session::{read, read_heads};
use crate::lan_session_push_tests::{StoppableLink, add_line, text_of};
use crate::lan_session_tests::drive_session;
use crate::server::SharedWorkspace;

/// Lines each device adds, in rounds that interleave the three.
const ROUNDS: usize = 30;

/// Links `a` and `b` with one live session each way; the drivers end when `stop` is set.
fn link(
    a: &SharedWorkspace,
    b: &SharedWorkspace,
    stop: &Arc<AtomicBool>,
) -> Vec<tokio::task::JoinHandle<bool>> {
    let (link_a, link_b) = txtodo_sync::channel_link_pair();
    [(link_a, Arc::clone(a)), (link_b, Arc::clone(b))]
        .into_iter()
        .map(|(inner, ws)| {
            let mut link = StoppableLink {
                inner,
                stop: Arc::clone(stop),
            };
            let (device, group) = (read(&ws).device(), read(&ws).group());
            tokio::task::spawn_blocking(move || drive_session(&mut link, ws, device, group))
        })
        .collect()
}

/// One mesh of three fresh devices: each adds [`ROUNDS`] lines, interleaved. `Err` names the heads
/// when they still differ after 10 s (`RESEND_AFTER` is 300 ms under test).
async fn mesh_once() -> Result<(), String> {
    let dirs: Vec<tempfile::TempDir> = (0..3).map(|_| tempfile::tempdir().unwrap()).collect();
    let a = device(dirs[0].path(), None);
    let (b, c) = (
        device(dirs[1].path(), Some(&a)),
        device(dirs[2].path(), Some(&a)),
    );
    let stop = Arc::new(AtomicBool::new(false));
    let mut drivers = link(&a, &b, &stop);
    drivers.extend(link(&b, &c, &stop));
    drivers.extend(link(&a, &c, &stop));
    let devices = [a, b, c];
    add_rounds(&devices).await;
    let converged = converged(&devices).await;
    stop.store(true, Ordering::Relaxed);
    for d in drivers {
        let _ = d.await;
    }
    converged?;
    let texts: Vec<_> = devices.iter().map(text_of).collect();
    assert!(texts.windows(2).all(|w| w[0] == w[1]), "{texts:#?}");
    let lines = texts[0].as_deref().unwrap_or_default().lines().count();
    assert_eq!(lines, 3 * ROUNDS, "every add on every device");
    Ok(())
}

/// Each device adds [`ROUNDS`] lines, the three taking turns.
async fn add_rounds(devices: &[SharedWorkspace]) {
    for round in 0..ROUNDS {
        for (i, ws) in devices.iter().enumerate() {
            add_line(ws, &format!("round {round} from {i}")).await;
        }
        tokio::time::sleep(Duration::from_millis(15)).await;
    }
}

/// Waits up to 10 s for every device to hold the same heads.
async fn converged(devices: &[SharedWorkspace]) -> Result<(), String> {
    let started = Instant::now();
    loop {
        let now: Vec<_> = devices.iter().map(read_heads).collect();
        if now.windows(2).all(|w| w[0] == w[1]) {
            return Ok(());
        }
        if started.elapsed() > Duration::from_secs(10) {
            return Err(format!("never converged: {now:?}"));
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// The stall is a race: three meshes in a row caught it nearly every time before the fix.
#[tokio::test(flavor = "multi_thread")]
async fn slow_three_linked_devices_adding_at_once_all_end_with_every_op() {
    for attempt in 0..3 {
        if let Err(why) = mesh_once().await {
            panic!("mesh {attempt}: {why}");
        }
    }
}
