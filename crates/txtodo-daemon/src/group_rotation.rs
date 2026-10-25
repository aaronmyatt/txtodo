//! Grants out and in (task `peer-prune`, ADR 0044): how a rotated group key reaches the devices
//! that were kept, and what a device does when one reaches it.
//!
//! **Out.** The key of every epoch is in the keystore, so a grant needs no store of its own: it is
//! made again, with `wrap_grant_for`, whenever a peer turns out to be behind. [`grants_for`] makes
//! the chain `peer+1..=ours`, each sealed under the epoch before it (the peer holds that one by the
//! time it reads the next). A device the rotation dropped gets none: that is the whole point.
//!
//! **In.** [`apply_grant`] opens a grant with this device's long-term key, files the key at its
//! epoch, moves the epoch, drops the devices the grant leaves out, and says what it did. Two
//! devices that rotate at once make two keys for one epoch; the lower minter id wins, and the other
//! adopts its key.
//! Ref: `docs/adr/0044-auto-prune-stale-peers.md`.

use txtodo_model::DeviceId;
use txtodo_sync::{
    ControlMessage, DeviceStaticPublic, KeyId, MAX_RETAINED_KEY_EPOCHS, Secret, wrap_grant_for,
};

use crate::device_identity::DeviceIdentity;
use crate::epoch_info::{self, EpochInfo};
use crate::group_epoch;

/// One grant to send, and the epoch its frame is sealed under.
pub(crate) struct OutGrant {
    /// The previous epoch: the receiver holds it before it reads this grant.
    pub(crate) sealed_under: u32,
    /// The `Grant`.
    pub(crate) message: ControlMessage,
}

/// The grants that bring `peer`, known to hold `peer_epoch`, up to this device's epoch, oldest
/// first. Empty when the peer is not behind, is unknown or removed, is too far behind to chain
/// (older than the retained epochs), or was dropped by one of the rotations in between (the chain
/// stops before it: it gets nothing from that epoch on).
pub(crate) fn grants_for(
    identity: &DeviceIdentity,
    peer: DeviceId,
    peer_epoch: u32,
) -> Vec<OutGrant> {
    let current = identity.group_epoch();
    let oldest = current.saturating_sub(u32::try_from(MAX_RETAINED_KEY_EPOCHS - 1).unwrap_or(0));
    if peer_epoch >= current || peer_epoch.saturating_add(1) < oldest {
        return Vec::new();
    }
    let store = identity.store().lock().unwrap_or_else(|e| e.into_inner());
    let Ok(Some(row)) = store.device(peer) else {
        return Vec::new();
    };
    if row.removed_at_ms.is_some() {
        return Vec::new();
    }
    let recipient = DeviceStaticPublic::from_bytes(row.static_public);
    let mut out = Vec::new();
    for epoch in peer_epoch + 1..=current {
        let info = epoch_info::read(&store, epoch).unwrap_or_else(|| EpochInfo {
            minter: identity.device(),
            left_out: Vec::new(),
        });
        if info.left_out.contains(&peer) {
            break;
        }
        let Ok(Some(key)) = group_epoch::fetch_raw(identity.key_store().as_ref(), epoch) else {
            break;
        };
        let Ok(wrapped) = wrap_grant_for(&key, epoch, &recipient) else {
            break;
        };
        out.push(OutGrant {
            sealed_under: epoch - 1,
            message: ControlMessage::grant(identity.device(), info.minter, wrapped, info.left_out),
        });
    }
    out
}

/// What [`apply_grant`] did.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Applied {
    /// The key is filed and this device is on `epoch` now.
    Adopted { epoch: u32 },
    /// This device already holds that key.
    Kept,
    /// Not applied, and why (a stable tag for the log).
    Ignored(&'static str),
}

/// What a grant for `epoch` means to a device on `current`, whose record for `epoch` (if it has one)
/// names `mine` as the minter. Pure, so the tie-break is tested on its own.
#[derive(Debug, PartialEq, Eq)]
enum Decision {
    Adopt,
    Already,
    Skip(&'static str),
}

fn decide(current: u32, epoch: u32, mine: Option<DeviceId>, minter: DeviceId) -> Decision {
    if epoch < current {
        return Decision::Skip("stale");
    }
    if epoch == current + 1 {
        return Decision::Adopt;
    }
    if epoch > current {
        return Decision::Skip("gap");
    }
    // The epoch this device is on: only a second key for it is a question, and the lower minter wins.
    match mine {
        Some(held) if held == minter => Decision::Already,
        Some(held) if minter < held => Decision::Adopt,
        Some(_) => Decision::Skip("mine_wins"),
        None => Decision::Skip("held_without_a_record"),
    }
}

/// Applies a received `Grant`. `now_ms` stamps the tombstones of the devices it leaves out.
pub(crate) fn apply_grant(identity: &DeviceIdentity, msg: &ControlMessage, now_ms: u64) -> Applied {
    let ControlMessage::Grant {
        sender,
        minter,
        epoch,
        left_out,
        ..
    } = msg
    else {
        return Applied::Ignored("not_a_grant");
    };
    let Some(wrapped) = msg.wrapped_grant() else {
        return Applied::Ignored("not_a_grant");
    };
    let mine = {
        let store = identity.store().lock().unwrap_or_else(|e| e.into_inner());
        // Only a device this one knows may move its epoch.
        match store.device(*sender) {
            Ok(Some(row)) if row.removed_at_ms.is_none() => {}
            _ => return Applied::Ignored("unknown_sender"),
        }
        epoch_info::read(&store, *epoch).map(|i| i.minter)
    };
    match decide(identity.group_epoch(), *epoch, mine, *minter) {
        Decision::Already => return Applied::Kept,
        Decision::Skip(why) => return Applied::Ignored(why),
        Decision::Adopt => {}
    }
    let Ok(key) = identity.open_grant(&wrapped) else {
        return Applied::Ignored("cannot_open");
    };
    match adopt(identity, (*sender, *minter, *epoch), &key, left_out, now_ms) {
        Ok(()) => Applied::Adopted { epoch: *epoch },
        Err(why) => Applied::Ignored(why),
    }
}

/// Files `key` at `epoch`, records who minted it, drops the devices left out, and moves the epoch:
/// the key first, so the epoch never points at a key this device does not hold.
fn adopt(
    identity: &DeviceIdentity,
    (sender, minter, epoch): (DeviceId, DeviceId, u32),
    key: &[u8; txtodo_sync::KEY_BYTES],
    left_out: &[DeviceId],
    now_ms: u64,
) -> Result<(), &'static str> {
    identity
        .key_store()
        .put(KeyId::Group(epoch), &Secret::new(key.to_vec()))
        .map_err(|_| "keystore")?;
    {
        let mut store = identity.store().lock().unwrap_or_else(|e| e.into_inner());
        let info = EpochInfo {
            minter,
            left_out: left_out.to_vec(),
        };
        let written = store
            .meta_set(
                crate::device_identity::GROUP_EPOCH_KEY,
                &epoch.to_be_bytes(),
            )
            .and_then(|()| epoch_info::write(&mut store, epoch, &info));
        written.map_err(|_| "store")?;
        for dropped in left_out
            .iter()
            .filter(|d| **d != identity.device() && **d != sender)
        {
            let _ = store.remove_device(*dropped, now_ms);
        }
        let _ = store.set_device_key_epoch(sender, epoch);
    }
    identity.set_group_epoch(epoch);
    Ok(())
}

/// Records that `peer` holds `epoch` (it said so, or it acknowledged a grant), never lowering what
/// is known. A peer this device has no row for is left alone.
pub(crate) fn note_peer_epoch(identity: &DeviceIdentity, peer: DeviceId, epoch: u32) {
    let mut store = identity.store().lock().unwrap_or_else(|e| e.into_inner());
    if let Ok(Some(row)) = store.device(peer)
        && row.removed_at_ms.is_none()
        && row.key_epoch < epoch
    {
        let _ = store.set_device_key_epoch(peer, epoch);
    }
}

/// The epoch this device last knew `peer` to hold, if the peer is a known, active device.
pub(crate) fn believed_epoch(identity: &DeviceIdentity, peer: DeviceId) -> Option<u32> {
    let store = identity.store().lock().unwrap_or_else(|e| e.into_inner());
    match store.device(peer) {
        Ok(Some(row)) if row.removed_at_ms.is_none() => Some(row.key_epoch),
        _ => None,
    }
}

/// Whether `peer` is a device this one removed.
pub(crate) fn is_removed(identity: &DeviceIdentity, peer: DeviceId) -> bool {
    let store = identity.store().lock().unwrap_or_else(|e| e.into_inner());
    matches!(store.device(peer), Ok(Some(row)) if row.removed_at_ms.is_some())
}

#[cfg(test)]
#[path = "group_rotation_tests.rs"]
mod tests;
