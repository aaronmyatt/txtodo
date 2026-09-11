#![no_main]
//! Fuzz `parse_line` in both modes and `tokenize`: lenient is total, strict agrees with lenient on the
//! fields whenever it accepts, spans cover every byte. Plan M1; run via `just fuzz parse_line <secs>`.
//! cargo-fuzz book: https://rust-fuzz.github.io/book/cargo-fuzz/tutorial.html
use libfuzzer_sys::fuzz_target;
use txtodo_core::{parse_line, tokenize, LineKind, Mode};

fuzz_target!(|data: &[u8]| {
    let Ok(s) = core::str::from_utf8(data) else { return };
    let lenient = parse_line(s, Mode::Lenient).expect("lenient must be total");
    // On a quirk-free line both modes must agree; on a quirky line lenient may read more structure
    // (e.g. `x 2026-09-11 (A) t`: strict sees description "(A) t", lenient sees priority A).
    if let (Ok(strict), LineKind::Task(lt), true) = (parse_line(s, Mode::Strict), &lenient.kind, lenient.quirks.is_empty()) {
        if let LineKind::Task(st) = strict.kind {
            assert_eq!(st.description, lt.description, "modes agree on the description when strict accepts");
            assert_eq!((st.completed, st.priority, st.creation_date), (lt.completed, lt.priority, lt.creation_date));
        }
    }
    let mut pos = 0;
    for span in tokenize(s) {
        assert_eq!(span.start, pos, "no gaps");
        assert!(span.end > span.start && s.is_char_boundary(span.end));
        pos = span.end;
    }
    assert_eq!(pos, s.len(), "spans cover the line");
});
