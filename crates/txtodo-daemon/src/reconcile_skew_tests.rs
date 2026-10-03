//! An editor save while a peer's stamps sit past the skew bound (lab clock-skew 1072683562): the
//! peer's lines carry stamps this device's clock never took, so its own next op is older than
//! them. The reconcile used to commit its render, placed as if newest; every replay of the log
//! placed the same ops by their real stamp, so this device's state left its own log, and the
//! peer's next op anchored on it split the file. Now the commit is what the log replays to.

use std::sync::Arc;

use txtodo_model::{
    DeviceId, FilePath, Hlc, IdentityMode, Op, OpId, OpKind, Principal, TaskId, Ulid,
};

use crate::clock::FakeClock;
use crate::log_repair::replay_from_empty;
use crate::sync_ops_tests::{open, store};

const NOW_MS: u64 = 1_000_000;
/// Ten minutes ahead: past the skew bound, so `observe_peer_stamps` does not merge it.
const AHEAD_MS: u64 = 600_000;

fn task(n: u128) -> TaskId {
    TaskId::new(Ulid::from_u128(0x7000 + n))
}

/// The peer's insert of line `n` after `after`, stamped ten minutes ahead.
fn peer_insert(n: u128, after: Option<u128>) -> Op {
    let device = DeviceId::new(Ulid::from_u128(2));
    Op {
        id: OpId::new(Ulid::from_u128(0x9000 + n)),
        hlc: Hlc {
            wall_ms: NOW_MS + AHEAD_MS,
            counter: u16::try_from(n).unwrap_or(0),
            device,
        },
        principal: Principal::User { device },
        file: FilePath::new("todo.txt").unwrap_or_else(|e| panic!("{e}")),
        kind: OpKind::Insert {
            task: task(n),
            after: after.map(task),
            line: format!("line {n} id:{}", task(n)),
        },
    }
}

#[test]
fn an_editor_move_of_a_line_stamped_ahead_commits_what_the_log_replays_to() {
    let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
    let store = store(dir.path());
    let clock = Arc::new(FakeClock::new(NOW_MS));
    let mut actor = open(dir.path(), &store, &clock);
    let peer = vec![
        peer_insert(1, None),
        peer_insert(2, Some(1)),
        peer_insert(3, Some(2)),
    ];
    actor.on_sync_ops(peer).unwrap_or_else(|e| panic!("{e}"));

    // The user moves line 3 to the top in an editor.
    let line = |n: u128| format!("line {n} id:{}\n", task(n));
    let saved = format!("{}{}{}", line(3), line(1), line(2));
    std::fs::write(dir.path().join("todo.txt"), &saved).unwrap_or_else(|e| panic!("{e}"));
    actor.on_external_change().unwrap_or_else(|e| panic!("{e}"));

    let path = FilePath::new("todo.txt").unwrap_or_else(|e| panic!("{e}"));
    let replayed = {
        let guard = store
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        replay_from_empty(&guard, &path, IdentityMode::Tagged)
            .unwrap_or_else(|e| panic!("{e}"))
            .unwrap_or_else(|| panic!("a replay"))
            .0
    };
    let live = String::from_utf8_lossy(&actor.projection).into_owned();
    let logged = String::from_utf8_lossy(&replayed.to_bytes()).into_owned();
    assert_eq!(
        live, logged,
        "this device's state is what its log replays to"
    );
    let on_disk = std::fs::read_to_string(dir.path().join("todo.txt")).unwrap_or_default();
    assert_eq!(on_disk, live, "and the file says so");
}
