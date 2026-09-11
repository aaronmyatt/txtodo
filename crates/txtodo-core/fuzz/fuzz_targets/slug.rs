#![no_main]
//! Fuzz the `ref:` slug validator: nothing it accepts can escape a directory (plan §5 security checklist).
use libfuzzer_sys::fuzz_target;
use txtodo_core::{is_valid_slug, SLUG_MAX_LEN};

fuzz_target!(|data: &[u8]| {
    let Ok(s) = core::str::from_utf8(data) else { return };
    if is_valid_slug(s) {
        assert!(!s.contains('/') && !s.contains('\\') && s != "." && s != "..", "traversal-safe: {s:?}");
        assert!(!s.is_empty() && s.len() <= SLUG_MAX_LEN && s.is_ascii(), "bounded ASCII: {s:?}");
        assert!(!s.starts_with(['.', '-', '_']), "starts with [a-z0-9]: {s:?}");
    }
});
