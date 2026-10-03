//! First sync to a new device (task first-sync-speed): three devices build a long history, then
//! fresh devices pull it over a real session, straight in per-origin and HLC order, and reopen.
//! Prints one `first-sync-bench:` line and fails only if a device does not converge. What each
//! phase means and the numbers: tasks/first-sync-speed/notes.md.
//!
//! `slow_`: out of the default nextest set. Run it with
//! `cargo nextest run -p txtodo-daemon --lib --profile ci -E 'test(first_sync_bench)' --no-capture`.
//! `TXTODO_BENCH_ROUNDS` scales the history (default [`ROUNDS`]).

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, PoisonError};
use std::time::{Duration, Instant};

use txtodo_core::Date;
use txtodo_model::{FilePath, Op, Principal};
use txtodo_sync::{Heads, MAX_OPS_PER_BATCH, channel_link_pair, want};

use crate::clock::{Clock, SystemClock};
use crate::handle::ActorHandle;
use crate::lan_apply::commit_incoming_ops;
use crate::lan_session::{read, read_heads, write};
use crate::lan_session_push_tests::StoppableLink;
use crate::lan_session_tests::drive_session;
use crate::mutation::{Mutation, TaskRef};
use crate::server::SharedWorkspace;

/// Rounds: every device makes [`ACTIONS`] edits, then all sync. 6 is ~20 s; see notes.md.
const ROUNDS: usize = 6;
const ACTIONS: usize = 40;
/// Sub-lists (`tasks/s<k>/todo.txt`, each with a `notes.md`).
const SUBLISTS: usize = 4;
const KEY: [u8; 32] = [7u8; 32];

/// xorshift64: a fixed, seedable sequence, so every run builds the same shape of history.
/// <https://en.wikipedia.org/wiki/Xorshift>
struct Rng(u64);

impl Rng {
    fn below(&mut self, n: usize) -> usize {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        usize::try_from(self.0 % n.max(1) as u64).unwrap_or(0)
    }
}

fn path(p: &str) -> FilePath {
    FilePath::new(p).unwrap_or_else(|e| panic!("{e}"))
}

/// A device on the real clock (`FakeClock`s started alike mint the same ids), joined to `lead`'s
/// group and workspace when given.
fn device(dir: &std::path::Path, lead: Option<&SharedWorkspace>) -> SharedWorkspace {
    std::fs::write(dir.join("todo.txt"), "").unwrap_or_else(|e| panic!("write: {e}"));
    let clock: Arc<dyn Clock> = Arc::new(SystemClock);
    let ws = crate::workspace::Workspace::open_with_default_mode(
        dir,
        clock,
        txtodo_model::IdentityMode::Tagged,
    )
    .unwrap_or_else(|e| panic!("open workspace: {e}"));
    ws.key_store()
        .put(
            txtodo_sync::KeyId::Group(0),
            &txtodo_sync::Secret::new(KEY.to_vec()),
        )
        .unwrap_or_else(|e| panic!("seed group key: {e}"));
    if let Some(lead) = lead {
        let lead = read(lead);
        ws.set_group(lead.group());
        ws.set_workspace_id(lead.workspace_id());
    }
    Arc::new(std::sync::RwLock::new(ws))
}

/// The actor for `p`, registering it (and making its directory) the first time.
fn actor(ws: &SharedWorkspace, p: &FilePath) -> ActorHandle {
    if let Some(h) = read(ws).actor(p) {
        return h.clone();
    }
    let disk = read(ws).root().join(p.as_str());
    if let Some(parent) = disk.parent() {
        std::fs::create_dir_all(parent).unwrap_or_else(|e| panic!("mkdir: {e}"));
    }
    let mut guard = write(ws);
    guard.register(p.clone()).unwrap_or_else(|e| panic!("{e}"));
    guard
        .actor(p)
        .cloned()
        .unwrap_or_else(|| panic!("no actor"))
}

fn bytes_of(ws: &SharedWorkspace, p: &FilePath) -> Vec<u8> {
    let handle = actor(ws, p);
    let rt = tokio::runtime::Handle::current();
    tokio::task::block_in_place(|| rt.block_on(handle.get()))
        .unwrap_or_else(|e| panic!("get: {e}"))
        .bytes
}

fn notes_of(ws: &SharedWorkspace, p: &FilePath) -> Vec<u8> {
    let cell = read(ws).notes_actor(p).unwrap_or_else(|e| panic!("{e}"));
    let actor = cell.lock().unwrap_or_else(PoisonError::into_inner);
    actor.contents().0
}

fn lines_of(ws: &SharedWorkspace, p: &FilePath) -> Vec<String> {
    let text = String::from_utf8(bytes_of(ws, p)).unwrap_or_default();
    text.lines().map(str::to_owned).collect()
}

fn line(n: usize) -> TaskRef {
    TaskRef {
        line_number: n + 1,
        task_id: None,
    }
}

/// One edit picked by `rng`, on the root list or a sub-list. A refused one (a `do` of a done
/// line, say) is fine: the history only needs to be long and tangled.
async fn act(ws: &SharedWorkspace, rng: &mut Rng, n: usize) {
    let list = match rng.below(3) {
        0 => path(&format!("tasks/s{}/todo.txt", rng.below(SUBLISTS))),
        _ => path("todo.txt"),
    };
    let lines = lines_of(ws, &list).len();
    let pick = rng.below(10);
    if pick == 9 {
        return edit_notes(ws, rng, n);
    }
    let mutation = match (pick, lines) {
        (0..=3, _) | (_, 0..=1) => Mutation::Add {
            line: format!("task {n} from {}", read(ws).device()),
        },
        (4..=5, _) => {
            let at = rng.below(lines);
            let old = lines_of(ws, &list).swap_remove(at);
            Mutation::Edit {
                task: line(at),
                new_line: format!("{old} +e{n}"),
            }
        }
        (6, _) => Mutation::Complete {
            task: line(rng.below(lines)),
            today: Date::new(2026, 10, 3).unwrap_or_else(|| panic!("date")),
        },
        _ => Mutation::MoveBefore {
            task: line(rng.below(lines)),
            before: line(rng.below(lines)),
        },
    };
    let device = read(ws).device();
    let _ = actor(ws, &list)
        .apply(vec![mutation], Principal::User { device })
        .await;
}

fn edit_notes(ws: &SharedWorkspace, rng: &mut Rng, n: usize) {
    let p = path(&format!("tasks/s{}/notes.md", rng.below(SUBLISTS)));
    let disk = read(ws).root().join(p.as_str());
    if let Some(parent) = disk.parent() {
        std::fs::create_dir_all(parent).unwrap_or_else(|e| panic!("mkdir: {e}"));
    }
    let old = String::from_utf8(notes_of(ws, &p)).unwrap_or_default();
    let device = read(ws).device();
    let cell = read(ws).notes_actor(&p).unwrap_or_else(|e| panic!("{e}"));
    let mut actor = cell.lock().unwrap_or_else(PoisonError::into_inner);
    let _ = actor.edit(&format!("{old}- note {n}\n"), Principal::User { device });
}

/// Every op `from` holds and `to` lacks: each origin's run in seq order, and the runs either
/// back to back (the sender's order today) or merged by HLC.
fn missing_ops(from: &SharedWorkspace, to: &SharedWorkspace, by_hlc: bool) -> Vec<Op> {
    let ranges = want(&read_heads(to), &read_heads(from));
    let store = read(from).store().clone();
    let store = store.lock().unwrap_or_else(PoisonError::into_inner);
    let step = u64::try_from(txtodo_store::MAX_OPS_PER_READ).unwrap_or(1);
    let mut runs: Vec<VecDeque<Op>> = Vec::new();
    for r in ranges {
        let mut run = VecDeque::new();
        for first in (r.first..=r.last).step_by(usize::try_from(step).unwrap_or(1)) {
            let last = (first + step - 1).min(r.last);
            let stored = store.ops_for(r.device, first, last);
            run.extend(
                stored
                    .unwrap_or_else(|e| panic!("{e}"))
                    .into_iter()
                    .map(|s| s.op),
            );
        }
        runs.push(run);
    }
    if !by_hlc {
        return runs.into_iter().flatten().collect();
    }
    let mut out = Vec::new();
    // Bounded: each pass takes one op off a run.
    while let Some(next) = runs
        .iter_mut()
        .filter(|r| !r.is_empty())
        .min_by_key(|r| r.front().map(|op| op.hlc))
    {
        out.extend(next.pop_front());
    }
    out
}

/// `from`'s ops `to` lacks, committed in `MAX_OPS_PER_BATCH` batches; every one must land.
async fn copy(from: &SharedWorkspace, to: &SharedWorkspace, by_hlc: bool) -> usize {
    let ops = missing_ops(from, to, by_hlc);
    let total = ops.len();
    let mut batches: Vec<Vec<Op>> = ops.chunks(MAX_OPS_PER_BATCH).map(<[Op]>::to_vec).collect();
    for batch in batches.drain(..) {
        let (to, rt, len) = (
            Arc::clone(to),
            tokio::runtime::Handle::current(),
            batch.len(),
        );
        let landed = tokio::task::spawn_blocking(move || commit_incoming_ops(&to, &rt, batch))
            .await
            .unwrap_or_else(|e| panic!("join: {e}"));
        assert_eq!(landed.ops, len, "refused: {:?}", landed.refused);
    }
    total
}

/// Every list and notes file on `a` that `b` does not hold byte for byte, one line each: whether
/// only the order differs. Heads must match.
fn differences(a: &SharedWorkspace, b: &SharedWorkspace, what: &str) -> Vec<String> {
    assert_eq!(read_heads(a), read_heads(b), "{what}: heads");
    let mut files: Vec<FilePath> = read(a).paths().cloned().collect();
    files.sort();
    let mut out = Vec::new();
    for p in &files {
        let (ours, theirs) = (bytes_of(a, p), bytes_of(b, p));
        if ours != theirs {
            out.push(format!(
                "{what}: {p} differs: {}",
                line_diff(&ours, &theirs)
            ));
        }
    }
    for k in 0..SUBLISTS {
        let p = path(&format!("tasks/s{k}/notes.md"));
        if notes_of(a, &p) != notes_of(b, &p) {
            out.push(format!("{what}: {p} differs"));
        }
    }
    out
}

/// The lines only one side holds (first few), or `order` when both hold the same lines.
fn line_diff(ours: &[u8], theirs: &[u8]) -> String {
    let lines = |b: &[u8]| -> Vec<String> {
        String::from_utf8_lossy(b)
            .lines()
            .map(str::to_owned)
            .collect()
    };
    let (ours, theirs) = (lines(ours), lines(theirs));
    let only = |x: &[String], y: &[String]| -> Vec<String> {
        x.iter()
            .filter(|l| !y.contains(l))
            .take(3)
            .cloned()
            .collect()
    };
    let (mine, yours) = (only(&ours, &theirs), only(&theirs, &ours));
    if mine.is_empty() && yours.is_empty() {
        return format!("order ({} lines)", ours.len());
    }
    format!("only on a: {mine:?}; only on b: {yours:?}")
}

/// Every device's ops to the first, then the first's to every other, in HLC order.
async fn sync_all(devices: &[SharedWorkspace]) {
    let a = &devices[0];
    for ws in &devices[1..] {
        copy(ws, a, true).await;
    }
    for ws in &devices[1..] {
        copy(a, ws, true).await;
    }
}

/// Three devices edit in turns and sync after every round (HLC order: nothing parks while the
/// history is built). Returns them, A first, all equal.
async fn build_history(dirs: &[tempfile::TempDir], rounds: usize) -> Vec<SharedWorkspace> {
    let a = device(dirs[0].path(), None);
    let mut devices = vec![Arc::clone(&a)];
    for dir in &dirs[1..] {
        devices.push(device(dir.path(), Some(&a)));
    }
    let mut rng = Rng(0x5eed_f00d);
    let mut n = 0;
    for _ in 0..rounds {
        for ws in &devices {
            for _ in 0..ACTIONS {
                n += 1;
                act(ws, &mut rng, n).await;
            }
        }
        sync_all(&devices).await;
    }
    for ws in &devices[1..] {
        let diff = differences(&a, ws, "history");
        assert!(diff.is_empty(), "{diff:?}");
    }
    devices
}

/// A fresh device synced from `a` over one real session; returns it once its heads match.
async fn session_sync(a: &SharedWorkspace, dir: &std::path::Path) -> SharedWorkspace {
    let fresh = device(dir, Some(a));
    let stop = Arc::new(AtomicBool::new(false));
    let (link_a, link_b) = channel_link_pair();
    let mut drivers = Vec::new();
    for (inner, ws) in [(link_a, Arc::clone(a)), (link_b, Arc::clone(&fresh))] {
        let mut link = StoppableLink {
            inner,
            stop: Arc::clone(&stop),
        };
        let (device, group) = (read(&ws).device(), read(&ws).group());
        drivers.push(tokio::task::spawn_blocking(move || {
            drive_session(&mut link, ws, device, group)
        }));
    }
    let want: Heads = read_heads(a);
    let started = Instant::now();
    while read_heads(&fresh) != want {
        assert!(started.elapsed() < Duration::from_secs(600), "never synced");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    stop.store(true, Ordering::Relaxed);
    for driver in drivers {
        let _ = driver.await;
    }
    fresh
}

fn ms(since: Instant) -> u128 {
    since.elapsed().as_millis()
}

#[tokio::test(flavor = "multi_thread")]
async fn slow_first_sync_bench() {
    let rounds = std::env::var("TXTODO_BENCH_ROUNDS")
        .ok()
        .and_then(|r| r.parse().ok())
        .unwrap_or(ROUNDS);
    let dirs: Vec<tempfile::TempDir> = (0..6).map(|_| tempfile::tempdir().unwrap()).collect();
    let started = Instant::now();
    let devices = build_history(&dirs[..3], rounds).await;
    let a = &devices[0];
    let build_ms = ms(started);

    let started = Instant::now();
    let fresh = session_sync(a, dirs[3].path()).await;
    let session_ms = ms(started);
    let mut diff = differences(a, &fresh, "session");
    let commits = read(&fresh).stats().commits();

    let by_origin = device(dirs[4].path(), Some(a));
    let started = Instant::now();
    let ops = copy(a, &by_origin, false).await;
    let origin_ms = ms(started);
    diff.extend(differences(a, &by_origin, "direct_origin"));

    let by_hlc = device(dirs[5].path(), Some(a));
    let started = Instant::now();
    copy(a, &by_hlc, true).await;
    let hlc_ms = ms(started);
    diff.extend(differences(a, &by_hlc, "direct_hlc"));

    drop(fresh);
    let started = Instant::now();
    let reopened = device_reopen(dirs[3].path());
    let reopen_ms = ms(started);
    diff.extend(differences(a, &reopened, "reopen"));
    eprintln!(
        "first-sync-bench: rounds={rounds} ops={ops} files={} build_ms={build_ms} \
         session_ms={session_ms} session_commits={commits} direct_origin_ms={origin_ms} \
         direct_hlc_ms={hlc_ms} reopen_ms={reopen_ms}",
        read(a).paths().count()
    );
    assert!(diff.is_empty(), "not converged: {diff:#?}");
}

/// The synced device's workspace opened again from disk, as a daemon restart would.
fn device_reopen(dir: &std::path::Path) -> SharedWorkspace {
    let clock: Arc<dyn Clock> = Arc::new(SystemClock);
    let ws = crate::workspace::Workspace::open_with_default_mode(
        dir,
        clock,
        txtodo_model::IdentityMode::Tagged,
    )
    .unwrap_or_else(|e| panic!("reopen workspace: {e}"));
    Arc::new(std::sync::RwLock::new(ws))
}
