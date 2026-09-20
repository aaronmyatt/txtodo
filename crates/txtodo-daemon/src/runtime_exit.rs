//! Leaving the process once the daemon's main future is done (code review 2026-09-20, finding 3).
//!
//! `GlobalService::resolve` parks on the blocking pool: a caller waiting for a workspace that is
//! still loading sits in a `Condvar` wait for up to `load_wait` (120 s), and an open runs there
//! whole. Dropping a plain `Runtime` waits for every blocking task that has started, with no bound,
//! so a `txtodod` told to stop could hang for two minutes. By then `run` has already released the
//! pid lock and removed the socket, so a new daemon could start beside the old one while it was
//! still opening a store. `shutdown_timeout` bounds that wait; a task still parked after it is
//! abandoned, and the process exit ends its thread.
//! https://docs.rs/tokio/latest/tokio/runtime/struct.Runtime.html#method.shutdown_timeout

use std::future::Future;
use std::time::Duration;
use tokio::runtime::Runtime;

/// How long a finished daemon waits for blocking tasks before it leaves them behind. Long enough
/// for a store commit in flight to land, short next to the 20 s launchd gives a job before SIGKILL.
pub const SHUTDOWN_GRACE: Duration = Duration::from_secs(5);

/// Runs `main_future` to its end on `rt`, then shuts the runtime down, waiting at most `grace` for
/// blocking tasks that are still running.
pub fn block_on_then_shut_down<F: Future>(
    rt: Runtime,
    main_future: F,
    grace: Duration,
) -> F::Output {
    let output = rt.block_on(main_future);
    rt.shutdown_timeout(grace);
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    /// A blocking task parked the way a `resolve` waiting on a loading workspace is. With a plain
    /// `drop(rt)` this test takes the full 60 s park.
    #[test]
    fn a_parked_blocking_task_does_not_hold_the_exit() {
        let rt = Runtime::new().unwrap_or_else(|e| panic!("runtime: {e}"));
        let (release, parked) = std::sync::mpsc::channel::<()>();
        let began = Instant::now();
        let output = block_on_then_shut_down(
            rt,
            async move {
                let (started_tx, started_rx) = tokio::sync::oneshot::channel::<()>();
                tokio::task::spawn_blocking(move || {
                    let _ = started_tx.send(());
                    let _ = parked.recv_timeout(Duration::from_secs(60));
                });
                // Only a blocking task that has started is waited for, so make sure it has.
                started_rx
                    .await
                    .unwrap_or_else(|e| panic!("blocking task never started: {e}"));
                7
            },
            Duration::from_millis(200),
        );
        assert_eq!(output, 7, "the main future's value comes back");
        assert!(
            began.elapsed() < Duration::from_secs(20),
            "the exit waited for the parked task: {:?}",
            began.elapsed()
        );
        drop(release); // held until here so the parked task could not end early
    }
}
