//! Why a device row was removed, and by whom (task `peer-prune`, ADR 0044): the clients tell "you
//! removed it" from "this device dropped it for being quiet" from "another device dropped it and we
//! followed". Kept in the identity store's `meta` as `removed_why:<device ulid>` = `<why>:<by ulid>`,
//! written in the same block that tombstones the row. A row removed before this existed has no record
//! and reads as "removed", the reason unknown. Pairing the device again brings the row back and the
//! record is not read until it is removed once more.

use txtodo_model::{DeviceId, Ulid};
use txtodo_store::{IdentityStore, StoreError};

use crate::prune_stale::Stale;

/// Why a row was removed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Why {
    /// A person ran `device remove`.
    Manual,
    /// This device dropped it: its keys match and it was not heard from for `prune_quiet_after`.
    PrunedQuiet,
    /// This device dropped it: its keys do not match and it was silent for `prune_parked_after`.
    PrunedParked,
    /// Another device dropped it and rotated the key; this device followed the grant.
    LeftOut,
}

impl Why {
    /// The word stored and sent to clients as `Device.removed_reason`.
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Why::Manual => "manual",
            Why::PrunedQuiet => "pruned_quiet",
            Why::PrunedParked => "pruned_parked",
            Why::LeftOut => "left_out",
        }
    }

    fn parse(word: &str) -> Option<Why> {
        [
            Why::Manual,
            Why::PrunedQuiet,
            Why::PrunedParked,
            Why::LeftOut,
        ]
        .into_iter()
        .find(|why| why.as_str() == word)
    }
}

impl From<Stale> for Why {
    fn from(stale: Stale) -> Why {
        match stale {
            Stale::Parked => Why::PrunedParked,
            Stale::Quiet => Why::PrunedQuiet,
        }
    }
}

/// The record of one removal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Removal {
    pub(crate) why: Why,
    /// The device that removed it: this one for a manual removal or a prune, the minter of the key
    /// for a row this device was told to drop.
    pub(crate) by: DeviceId,
}

fn key(device: DeviceId) -> String {
    format!("removed_why:{}", device.ulid())
}

/// Records that `device` was removed for `why` by `by`.
pub(crate) fn write(
    store: &mut IdentityStore,
    device: DeviceId,
    removal: Removal,
) -> Result<(), StoreError> {
    let value = format!("{}:{}", removal.why.as_str(), removal.by.ulid());
    store.meta_set(&key(device), value.as_bytes())
}

/// The record for `device`, if one was written.
pub(crate) fn read(store: &IdentityStore, device: DeviceId) -> Option<Removal> {
    let bytes = store.meta_get(&key(device)).ok().flatten()?;
    let text = String::from_utf8(bytes).ok()?;
    let (word, by) = text.split_once(':')?;
    // The ULID spec's 26-character Crockford text form (`txtodo_core::Ulid::parse`).
    // Ref: https://github.com/ulid/spec#canonical-string-representation
    let by = DeviceId::new(Ulid::parse(by)?);
    Some(Removal {
        why: Why::parse(word)?,
        by,
    })
}

#[cfg(test)]
#[path = "removal_why_tests.rs"]
mod tests;
