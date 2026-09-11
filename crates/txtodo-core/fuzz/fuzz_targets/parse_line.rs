#![no_main]
//! Fuzz target `parse_line`. Stub until M1 proves the harness end to end; M1 replaces the body with
//! `let _ = txtodo_core::parse_line(s, Mode::Strict)` and `Mode::Lenient` (plan M1 tasks).
//! cargo-fuzz book: https://rust-fuzz.github.io/book/cargo-fuzz/tutorial.html
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // Only valid UTF-8 can be a todo.txt line; invalid input is simply not a line.
    let _ = core::str::from_utf8(data);
});
