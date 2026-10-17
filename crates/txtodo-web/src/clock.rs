//! Time and entropy for the host: Unix milliseconds and fresh ULIDs. In a browser the clock is
//! `Date.now()` (std's `SystemTime::now` panics on `wasm32-unknown-unknown`), and getrandom reads
//! `crypto.getRandomValues`.

use txtodo_model::Ulid;

/// Unix milliseconds now.
/// Ref: <https://docs.rs/js-sys/latest/js_sys/struct.Date.html#method.now>
#[cfg(all(target_family = "wasm", target_os = "unknown"))]
pub fn now_ms() -> u64 {
    // A JS number of milliseconds; whole and non-negative for any real clock.
    js_sys::Date::now().max(0.0) as u64
}

/// Unix milliseconds now.
/// Ref: <https://doc.rust-lang.org/std/time/struct.SystemTime.html>
#[cfg(not(all(target_family = "wasm", target_os = "unknown")))]
pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

/// A ULID at `ms` with 80 random bits: 48 bits of time, then the randomness, as the daemon's
/// `clock::ulid_from` builds one. Ref: <https://github.com/ulid/spec>
pub fn new_ulid(ms: u64) -> Ulid {
    let mut random = [0u8; 10];
    // Entropy failure is not recoverable in a meaningful way; the time half still orders ids.
    if getrandom::fill(&mut random).is_err() {
        random = [0xA5; 10];
    }
    let bits = random
        .iter()
        .fold(0u128, |acc, b| (acc << 8) | u128::from(*b));
    let ms48 = u128::from(ms) & ((1u128 << 48) - 1);
    Ulid::from_u128((ms48 << 80) | bits)
}
