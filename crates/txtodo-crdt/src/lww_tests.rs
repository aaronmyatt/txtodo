//! LWW register tests: encode/decode round-trip and the higher HLC wins regardless of apply order.

use loro::{LoroMap, LoroValue};
use txtodo_model::{DeviceId, Hlc, Ulid};

use crate::{Lww, write_if_newer};

fn dev() -> DeviceId {
    DeviceId::new(Ulid::from_u128(7))
}

fn hlc(n: u64) -> Hlc {
    Hlc {
        wall_ms: n,
        counter: 0,
        device: dev(),
    }
}

fn read_str(map: &LoroMap, key: &str) -> Option<String> {
    let voc = map.get(key)?;
    let v = voc.into_value().ok()?;
    let lww = Lww::decode(&v)?;
    lww.value.as_string().map(|s| s.as_ref().to_owned())
}

fn writes(map: &LoroMap, key: &str, value: &str, stamp: Hlc) -> bool {
    matches!(
        write_if_newer(map, key, LoroValue::from(value), stamp),
        Ok(true)
    )
}

#[test]
fn lww_encode_decode_round_trips() {
    let stamp = hlc(1_000);
    let reg = Lww {
        value: LoroValue::from("done"),
        hlc: stamp,
    };
    let decoded = Lww::decode(&reg.encode());
    assert_eq!(decoded, Some(reg));
}

#[test]
fn higher_hlc_wins_regardless_of_apply_order() {
    let low = hlc(1_000);
    let high = hlc(1_001);
    let a = LoroMap::new();
    assert!(writes(&a, "k", "low", low));
    assert!(writes(&a, "k", "high", high));
    assert_eq!(read_str(&a, "k").as_deref(), Some("high"));

    let b = LoroMap::new();
    assert!(writes(&b, "k", "high", high));
    assert!(!writes(&b, "k", "low", low));
    assert_eq!(read_str(&b, "k").as_deref(), Some("high"));
}

#[test]
fn wins_over_is_strictly_newer() {
    let reg = Lww {
        value: LoroValue::Null,
        hlc: hlc(5),
    };
    assert!(reg.wins_over(hlc(6)));
    assert!(!reg.wins_over(hlc(5)));
    assert!(!reg.wins_over(hlc(4)));
}
