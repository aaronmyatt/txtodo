#![no_main]
//! Fuzz `parse_file`: any bytes round-trip exactly through `to_bytes` (design §2.2 rules 2, 6, 7).
use libfuzzer_sys::fuzz_target;
use txtodo_core::parse_file;

fuzz_target!(|data: &[u8]| {
    let file = parse_file(data);
    assert_eq!(file.to_bytes(), data, "byte-for-byte round trip");
    assert!(file.lines.iter().filter(|l| l.ending() == txtodo_core::LineEnding::None).count() <= 1);
});
