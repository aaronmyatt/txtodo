//! Last-writer-wins register stamped by our HLC inside the value (ADR 0013).
//!
//! Loro resolves map-key conflicts with its own Lamport clock + peer id, which is not our HLC.
//! To keep design §4.2 honest, the stamp lives in the value: each field key holds
//! `{"v": value, "h": <26-byte HLC>}` and we write only when the incoming HLC is strictly newer.
//! Loro's own LWW then only breaks ties we never rely on. Loro map API:
//! <https://loro.dev/docs/tutorial/map> · HLC: <https://cse.buffalo.edu/tech-reports/2014-04.pdf>.

use std::collections::HashMap;

use loro::{LoroMap, LoroResult, LoroValue};
use txtodo_core::Ulid;
use txtodo_model::{DeviceId, Hlc};

/// Bytes in the binary HLC stamp: u64 wall LE + u16 counter LE + u128 device LE.
const HLC_STAMP_BYTES: usize = 26;

/// A last-writer-wins register: a value plus the HLC stamp that arbitrates which writer wins.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Lww<T> {
    /// The stored value.
    pub value: T,
    /// The stamp that decides whether an incoming write replaces this one.
    pub hlc: Hlc,
}

impl<T> Lww<T> {
    /// True when `incoming` is at least as new as this register's stamp. Across devices `Hlc`'s
    /// derived `Ord` breaks `(wall_ms, counter)` ties by device id, so a true tie only happens
    /// within one device's batch (one HLC tick per batch, ADR 0013): there the later write must
    /// land, which is Loro's own last-writer order — the tiebreak the ADR said we lean on.
    pub fn wins_over(&self, incoming: Hlc) -> bool {
        incoming >= self.hlc
    }
}

impl Lww<LoroValue> {
    /// Encodes as a `LoroValue::Map` `{"v": value, "h": <26-byte HLC>}`.
    pub fn encode(&self) -> LoroValue {
        let mut map = HashMap::with_capacity(2);
        map.insert("v".to_owned(), self.value.clone());
        map.insert("h".to_owned(), LoroValue::from(hlc_to_bytes(self.hlc)));
        LoroValue::Map(map.into())
    }

    /// Decodes the value written by [`Lww::encode`]; `None` on any malformed shape.
    pub fn decode(v: &LoroValue) -> Option<Lww<LoroValue>> {
        let map = v.as_map()?;
        let value = map.get("v")?.clone();
        let h = map.get("h")?.as_binary()?;
        Some(Lww {
            value,
            hlc: hlc_from_bytes(&h[..])?,
        })
    }
}

/// Reads, decodes and compares the register at `key`, writing `value` only when `incoming` is
/// strictly newer than what is stored. Returns whether a write happened.
pub fn write_if_newer(
    map: &LoroMap,
    key: &str,
    value: LoroValue,
    incoming: Hlc,
) -> LoroResult<bool> {
    let existing = map
        .get(key)
        .and_then(|voc| voc.into_value().ok())
        .and_then(|v| Lww::decode(&v));
    let wins = existing.is_none_or(|reg| reg.wins_over(incoming));
    if wins {
        map.insert(
            key,
            Lww {
                value,
                hlc: incoming,
            }
            .encode(),
        )?;
        Ok(true)
    } else {
        Ok(false)
    }
}

/// Encodes an [`Hlc`] as 26 little-endian bytes (wall_ms, counter, device).
fn hlc_to_bytes(h: Hlc) -> Vec<u8> {
    let mut b = Vec::with_capacity(HLC_STAMP_BYTES);
    b.extend_from_slice(&h.wall_ms.to_le_bytes());
    b.extend_from_slice(&h.counter.to_le_bytes());
    b.extend_from_slice(&h.device.ulid().to_u128().to_le_bytes());
    debug_assert_eq!(b.len(), HLC_STAMP_BYTES);
    b
}

/// Decodes the 26-byte stamp produced by [`hlc_to_bytes`]; `None` on the wrong length.
fn hlc_from_bytes(b: &[u8]) -> Option<Hlc> {
    if b.len() != HLC_STAMP_BYTES {
        return None;
    }
    let wall_ms = u64::from_le_bytes(b[0..8].try_into().ok()?);
    let counter = u16::from_le_bytes(b[8..10].try_into().ok()?);
    let device = u128::from_le_bytes(b[10..26].try_into().ok()?);
    Some(Hlc {
        wall_ms,
        counter,
        device: DeviceId::new(Ulid::from_u128(device)),
    })
}
