//! Plan M3 crash safety: kill -9 the daemon mid-write. After a restart the file holds the old or
//! the new projection (never a prefix), SQLite is consistent, seqs are dense, and the daemon comes
//! back ready. Seeded random kill delays; the seed is printed on failure.
// Integration tests are tests: clippy.toml allows unwrap/expect in #[test] fns but not in their helpers.
#![allow(clippy::expect_used, clippy::unwrap_used)]
#![cfg(unix)]

mod support;

use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use txtodo_store::Store;

/// Rounds of start → edit → kill → inspect.
const CRASH_ITERATIONS: u32 = 4;
/// Lines in the fixture; large enough that a write takes measurable time.
const LINES: usize = 10_000;
/// Longest random delay before the kill.
const MAX_KILL_DELAY_MS: u64 = 120;

/// Tiny deterministic PRNG (xorshift64*); no dependency, reproducible from the seed.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
}

fn fixture() -> String {
    (0..LINES)
        .map(|i| format!("task {i} +p{} @c{}\n", i % 7, i % 3))
        .collect()
}

fn spawn(dir: &Path) -> std::process::Child {
    let child = Command::new(env!("CARGO_BIN_EXE_txtodod"))
        .args(["--dir", &dir.to_string_lossy()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap_or_else(|e| panic!("spawn: {e}"));
    let socket = dir.join(".txtodo").join("txtodod.sock");
    let start = Instant::now();
    while !socket.exists() {
        assert!(
            start.elapsed() < Duration::from_secs(30),
            "socket did not appear"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    child
}

/// Waits until the daemon's adoption/reconcile write has landed: every line carries an id.
fn wait_stamped(dir: &Path) -> String {
    let start = Instant::now();
    loop {
        let text = std::fs::read_to_string(dir.join("todo.txt")).unwrap_or_default();
        if !text.is_empty() && text.lines().all(|l| l.is_empty() || l.contains(" id:")) {
            return text;
        }
        assert!(
            start.elapsed() < Duration::from_secs(30),
            "daemon never stamped ids"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn kill_nine_mid_write_leaves_old_or_new_projection_and_a_consistent_log() {
    let seed = 0x5EED_2026_0911u64;
    let mut rng = Rng(seed);
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("todo.txt"), fixture()).unwrap();
    let mut child = spawn(dir.path());
    let mut stamped = wait_stamped(dir.path());
    for round in 0..CRASH_ITERATIONS {
        // An external edit that strips every id and touches every line: the reconcile must mint
        // ids and write the whole file back — a large write to crash in the middle of.
        let edited: String = stamped
            .lines()
            .map(|l| {
                let text = l.split(" id:").next().unwrap_or(l);
                format!("{} r{round}\n", text.trim_end())
            })
            .collect();
        std::fs::write(dir.path().join("todo.txt"), &edited).unwrap();
        let delay = rng.next() % MAX_KILL_DELAY_MS;
        std::thread::sleep(Duration::from_millis(200 + delay));
        child.kill().unwrap();
        let _ = child.wait();
        let on_disk = std::fs::read_to_string(dir.path().join("todo.txt")).unwrap();
        let store = Store::open(&dir.path().join(".txtodo").join("oplog.db"))
            .unwrap_or_else(|e| panic!("open store: {e}"));
        assert!(
            store.integrity_ok().unwrap(),
            "seed {seed:#x} round {round}: integrity_check failed"
        );
        assert!(
            store.seqs_are_contiguous().unwrap(),
            "seed {seed:#x} round {round}: seq hole"
        );
        drop(store);
        let all_stamped = on_disk.lines().all(|l| l.contains(" id:"));
        assert!(
            on_disk == edited || (on_disk.lines().count() == LINES && all_stamped),
            "seed {seed:#x} round {round}: partial file ({} lines, stamped {all_stamped})",
            on_disk.lines().count()
        );
        child = spawn(dir.path());
        stamped = wait_stamped(dir.path());
        assert_eq!(
            stamped.lines().count(),
            LINES,
            "seed {seed:#x} round {round}: line count after restart"
        );
        assert!(
            stamped
                .lines()
                .all(|l| l.contains(&format!(" r{round} id:"))),
            "seed {seed:#x} round {round}: the edit survived"
        );
    }
    let _ = child.kill();
    let _ = child.wait();
}
