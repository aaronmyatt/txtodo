//! Task sync-drift line 7 over a real session: B cannot take A's ops for one file (a plain file
//! sits where its folder would go), so B books where sync from A is stuck. Once the folder can be
//! made, A's resend lands and the record clears. `RESEND_AFTER` is 300 ms under test.

use std::sync::{Arc, PoisonError};

use txtodo_model::{FilePath, Principal};

use crate::lan_session_resend_tests::{OPS_FRAME_MIN, StopOnDrop, finish, start_pair, wait_until};
use crate::mutation::Mutation;
use crate::server::SharedWorkspace;

/// Adds `line` to `file` on `ws`, making the document first.
async fn add_to(ws: &SharedWorkspace, file: &str, line: &str) {
    let path = FilePath::new(file).unwrap_or_else(|e| panic!("{e}"));
    let (handle, device) = {
        let mut guard = ws.write().unwrap_or_else(PoisonError::into_inner);
        let disk = guard.root().join(file);
        std::fs::create_dir_all(disk.parent().unwrap_or_else(|| panic!("a parent")))
            .unwrap_or_else(|e| panic!("mkdir: {e}"));
        std::fs::write(&disk, "").unwrap_or_else(|e| panic!("write: {e}"));
        guard
            .register(path.clone())
            .unwrap_or_else(|e| panic!("register: {e}"));
        let handle = guard.actor(&path).cloned();
        (handle.unwrap_or_else(|| panic!("an actor")), guard.device())
    };
    let add = Mutation::Add {
        line: line.to_owned(),
    };
    handle
        .apply(vec![add], Principal::User { device })
        .await
        .unwrap_or_else(|e| panic!("apply: {e}"));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_run_the_peer_cannot_take_is_booked_as_stuck_and_clears_once_it_lands() {
    let pair = start_pair(OPS_FRAME_MIN);
    let _stop = StopOnDrop(Arc::clone(&pair.stop));
    let (a, b, device_b) = (Arc::clone(&pair.a), Arc::clone(&pair.b), pair.device_b);
    wait_until("never linked", || {
        a.read().unwrap().live_peers().is_live(device_b)
    })
    .await;
    let device_a = a.read().unwrap().device();
    let blocked = b.read().unwrap().root().join("blocked");
    std::fs::write(&blocked, "").unwrap_or_else(|e| panic!("write: {e}"));

    add_to(&a, "blocked/todo.txt", "cannot land yet").await;
    let stuck = || b.read().unwrap().stuck_sync().of(device_a);
    wait_until("B never booked the refusal", || !stuck().is_empty()).await;
    let rows = stuck();
    assert_eq!(rows[0].1.file.as_str(), "blocked/todo.txt");
    assert!(rows[0].1.reason.starts_with("mkdir: "), "{:?}", rows[0]);

    std::fs::remove_file(&blocked).unwrap_or_else(|e| panic!("rm: {e}"));
    wait_until("the resend never landed", || stuck().is_empty()).await;
    let landed = std::fs::read_to_string(b.read().unwrap().root().join("blocked/todo.txt"));
    assert!(landed.unwrap_or_default().contains("cannot land yet"));
    finish(pair).await;
}
