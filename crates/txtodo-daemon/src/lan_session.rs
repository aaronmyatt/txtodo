//! Crypto-material lookups shared by every real `Link`-driving path in this crate (plan M4
//! `sync-lan-transport`; task `daemon-workspace-session-multiplex` stage 2 split this file down to
//! just these): fetching a workspace's group key, building its single-epoch `GroupKeys`, and
//! reading its current heads. `lan_session_dispatch.rs` is where the actual multiplexed read/write
//! loop lives (`drive_shared_session`). The single-workspace `drive_session` wrapper is gone (task
//! `sync-live-push`): LAN now drives every connection over the device's own route table, and the
//! tests' one-workspace helper lives in `lan_session_tests.rs`.

use std::sync::PoisonError;

use txtodo_sync::GroupKeys;

use crate::group_epoch::{self, EpochKey};

use crate::server::SharedWorkspace;
use crate::workspace::Workspace;

pub(crate) fn read(ws: &SharedWorkspace) -> std::sync::RwLockReadGuard<'_, Workspace> {
    ws.read().unwrap_or_else(PoisonError::into_inner)
}

pub(crate) fn write(ws: &SharedWorkspace) -> std::sync::RwLockWriteGuard<'_, Workspace> {
    ws.write().unwrap_or_else(PoisonError::into_inner)
}

/// The current epoch's group key (task `peer-prune`: the epoch moves with a rotation, so it is read
/// off the device each time, never a constant). `pub(crate)`: `file_carrier.rs` (plan M8
/// `relay-converge-test`) reuses this too — the file carrier's send/receive path needs the same
/// group key `drive_shared_session` does, with no `Session` of its own (see that module's doc for
/// why it does not reuse the driver wholesale). Every caller's own "no key" skip stays one flat
/// debug event (`lan_session_dispatch.rs`'s `lan_session_skipped_no_group_key`,
/// `control_session.rs`'s own copy) — fixed here instead, at the source, so each of the three
/// distinct failures logs its own specific reason before this still returns the same `None` every
/// caller already handles.
pub(crate) fn fetch_group_key(ws: &SharedWorkspace) -> Option<EpochKey> {
    let (epoch, store) = {
        let guard = read(ws);
        (guard.group_epoch(), guard.key_store().clone())
    };
    match group_epoch::fetch(store.as_ref(), epoch) {
        Ok(Some(key)) => Some(EpochKey { epoch, key }),
        Ok(None) => log_group_key_missing(),
        Err(why) => log_group_key_unreadable(&why),
    }
}

fn log_group_key_unreadable(why: &str) -> Option<EpochKey> {
    tracing::warn!(why, "lan_session_group_key_unreadable");
    None
}

fn log_group_key_missing() -> Option<EpochKey> {
    tracing::debug!("lan_session_group_key_missing");
    None
}

/// Every epoch this device holds up to `current` (`group_epoch::retained`): what incoming frames
/// open under. `pub(crate)`: see [`fetch_group_key`]'s doc.
pub(crate) fn retained_keys(ws: &SharedWorkspace, current: u32) -> Option<GroupKeys> {
    let store = read(ws).key_store().clone();
    group_epoch::retained(store.as_ref(), current)
}

/// `pub(crate)`: see [`fetch_group_key`]'s doc.
pub(crate) fn read_heads(ws: &SharedWorkspace) -> txtodo_sync::Heads {
    let store = read(ws).store().clone();
    store
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .heads()
        .unwrap_or_default()
}
