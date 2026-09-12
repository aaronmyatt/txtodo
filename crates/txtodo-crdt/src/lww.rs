//! LWW registers: a value plus the HLC that wrote it (ADR 0013).
//!
//! Loro resolves map-key conflicts by its own Lamport clock and peer id, which is not our HLC and
//! which we cannot substitute. We therefore store our stamp beside the value and write only when the
//! incoming stamp is strictly newer, so the merge rule is ours and stays readable in one place.
//! Ref: <https://docs.rs/loro/1.16.0/loro/struct.LoroMap.html>

use loro::{LoroMap, LoroMapValue, LoroResult, LoroValue, ValueOrContainer};
use std::collections::HashMap;
use txtodo_model::{DeviceId, Hlc, Ulid};

/// Length of an encoded HLC: `wall_ms` (u64) + `counter` (u16) + `device` (u128), little-endian.
pub const HLC_BYTES: usize = 26;

/// Key holding the register's value inside the encoded map.
const VALUE_KEY: &str = "v";
/// Key holding the register's HLC bytes inside the encoded map.
const HLC_KEY: &str = "h";

/// A last-writer-wins register.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Lww<T> {
    /// The value.
    pub value: T,
    /// The stamp that wrote it.
    pub hlc: Hlc,
}

impl<T> Lww<T> {
    /// Builds a register from a value and the stamp that wrote it.
    pub fn new(value: T, hlc: Hlc) -> Lww<T> {
        debug_assert_eq!(HLC_BYTES, encode_hlc(hlc).len());
        Lww { value, hlc }
    }

    /// True when `incoming` must replace this register.
    ///
    /// `Hlc`'s `Ord` is `(wall_ms, counter, device)`, so the device id breaks an exact tie — the
    /// deterministic last resort ADR 0013 asks for.
    pub fn wins_over(&self, incoming: Hlc) -> bool {
        let wins = incoming > self.hlc;
        debug_assert!(
            !(wins && incoming == self.hlc),
            "only a strictly newer stamp wins"
        );
        wins
    }
}

/// Encodes an HLC to its fixed [`HLC_BYTES`]-byte little-endian form.
pub fn encode_hlc(hlc: Hlc) -> Vec<u8> {
    let mut out = Vec::with_capacity(HLC_BYTES);
    out.extend_from_slice(&hlc.wall_ms.to_le_bytes());
    out.extend_from_slice(&hlc.counter.to_le_bytes());
    out.extend_from_slice(&hlc.device.ulid().to_u128().to_le_bytes());
    debug_assert_eq!(out.len(), HLC_BYTES, "fixed-width encoding");
    out
}

/// Decodes the bytes written by [`encode_hlc`]; `None` when the length is wrong.
pub fn decode_hlc(bytes: &[u8]) -> Option<Hlc> {
    if bytes.len() != HLC_BYTES {
        return None;
    }
    let wall_ms = u64::from_le_bytes(bytes.get(0..8)?.try_into().ok()?);
    let counter = u16::from_le_bytes(bytes.get(8..10)?.try_into().ok()?);
    let device = u128::from_le_bytes(bytes.get(10..26)?.try_into().ok()?);
    let hlc = Hlc {
        wall_ms,
        counter,
        device: DeviceId::new(Ulid::from_u128(device)),
    };
    debug_assert_eq!(
        Ulid::from_u128(device).to_u128(),
        device,
        "device round-trips"
    );
    Some(hlc)
}

impl<T: Clone + Into<LoroValue>> Lww<T> {
    /// Encodes to a `LoroValue::Map` of `{"v": value, "h": <26 bytes>}`.
    pub fn encode(&self) -> LoroValue {
        let mut map = HashMap::with_capacity(2);
        map.insert(VALUE_KEY.to_owned(), self.value.clone().into());
        map.insert(HLC_KEY.to_owned(), LoroValue::from(encode_hlc(self.hlc)));
        let value = LoroValue::Map(LoroMapValue::from(map));
        debug_assert!(matches!(value, LoroValue::Map(_)), "always a map value");
        value
    }
}

/// Decodes a register written by [`Lww::encode`]; `None` on any other shape.
pub fn decode(value: &LoroValue) -> Option<Lww<LoroValue>> {
    let LoroValue::Map(map) = value else {
        return None;
    };
    let inner = map.get(VALUE_KEY)?.clone();
    let LoroValue::Binary(binary) = map.get(HLC_KEY)? else {
        return None;
    };
    let hlc = decode_hlc(binary)?;
    let register = Lww::new(inner, hlc);
    debug_assert_eq!(encode_hlc(register.hlc), binary.to_vec());
    Some(register)
}

/// Reads the register stored at `key`, if it is one.
pub fn read(map: &LoroMap, key: &str) -> Option<Lww<LoroValue>> {
    let ValueOrContainer::Value(value) = map.get(key)? else {
        return None;
    };
    let register = decode(&value);
    debug_assert!(
        register.is_none() || map.get(key).is_some(),
        "reading a register never mutates the map"
    );
    register
}

/// Writes `value` at `key` only when `incoming` beats the stored stamp. Returns whether it wrote.
///
/// Ref: <https://docs.rs/loro/1.16.0/loro/struct.LoroMap.html#method-insert>
pub fn write_if_newer(
    map: &LoroMap,
    key: &str,
    value: LoroValue,
    incoming: Hlc,
) -> LoroResult<bool> {
    let newer = read(map, key).is_none_or(|existing| existing.wins_over(incoming));
    if newer {
        map.insert(key, Lww::new(value, incoming).encode())?;
    }
    debug_assert!(
        newer || read(map, key).is_some(),
        "a skipped write leaves one"
    );
    Ok(newer)
}

#[cfg(test)]
mod tests {
    use super::*;
    use loro::LoroDoc;
    use txtodo_model::Ulid;

    fn device(n: u128) -> DeviceId {
        DeviceId::new(Ulid::from_u128(n))
    }

    fn hlc(wall_ms: u64, counter: u16, dev: u128) -> Hlc {
        Hlc {
            wall_ms,
            counter,
            device: device(dev),
        }
    }

    #[test]
    fn hlc_round_trips_through_bytes() {
        let stamp = hlc(1_700_000_000_000, 7, 0xAB);
        let bytes = encode_hlc(stamp);
        assert_eq!(bytes.len(), HLC_BYTES);
        assert_eq!(decode_hlc(&bytes), Some(stamp));
        assert_eq!(decode_hlc(&bytes[..HLC_BYTES - 1]), None);
        assert_eq!(decode_hlc(&[]), None);
    }

    #[test]
    fn register_round_trips_and_reports_the_stamp() {
        let stamp = hlc(5, 1, 2);
        let encoded = Lww::new(true, stamp).encode();
        let decoded = decode(&encoded).expect("encoded register decodes");
        assert_eq!(decoded.hlc, stamp);
        assert_eq!(decoded.value, LoroValue::Bool(true));
        assert_eq!(decode(&LoroValue::Null), None);
    }

    #[test]
    fn wins_over_is_strict_and_breaks_ties_by_device() {
        let base = hlc(10, 0, 1);
        let register = Lww::new(true, base);
        assert!(register.wins_over(hlc(11, 0, 1)), "newer wall wins");
        assert!(register.wins_over(hlc(10, 1, 1)), "newer counter wins");
        assert!(register.wins_over(hlc(10, 0, 2)), "device breaks the tie");
        assert!(!register.wins_over(base), "equal loses");
        assert!(!register.wins_over(hlc(9, 9, 9)), "older loses");
    }

    #[test]
    fn write_if_newer_writes_once_and_skips_older_and_equal() {
        let doc = LoroDoc::new();
        let map = doc.get_map("tasks");
        let stamp = hlc(10, 0, 1);
        assert!(write_if_newer(&map, "completed", LoroValue::Bool(true), stamp).unwrap());
        assert_eq!(read(&map, "completed").map(|r| r.hlc), Some(stamp));
        // An equal stamp is a replay, not a change.
        assert!(!write_if_newer(&map, "completed", LoroValue::Bool(false), stamp).unwrap());
        assert_eq!(
            read(&map, "completed").map(|r| r.value),
            Some(LoroValue::Bool(true))
        );
        // A newer stamp overwrites.
        let newer = hlc(11, 0, 1);
        assert!(write_if_newer(&map, "completed", LoroValue::Bool(false), newer).unwrap());
        assert_eq!(
            read(&map, "completed").map(|r| r.value),
            Some(LoroValue::Bool(false))
        );
        // An older stamp is ignored.
        assert!(!write_if_newer(&map, "completed", LoroValue::Bool(true), stamp).unwrap());
        assert_eq!(
            read(&map, "completed").map(|r| r.value),
            Some(LoroValue::Bool(false))
        );
    }
}
