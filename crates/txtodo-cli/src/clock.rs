//! The two things the CLI takes from the outside world besides files: today's local date and fresh
//! ULIDs. Both are called only at the edge (`main`'s dispatch); command logic receives values.

use std::io;
use std::time::{SystemTime, UNIX_EPOCH};
use txtodo_core::{Date, Ulid};

/// Today in the local time zone, `YYYY-MM-DD` (plan §1 decision 11: local date, never a time zone).
/// https://docs.rs/jiff/latest/jiff/struct.Zoned.html#method.now
pub fn today_local() -> Date {
    let d = jiff::Zoned::now().date();
    let year = u16::try_from(d.year()).unwrap_or(0);
    let month = u8::try_from(d.month()).unwrap_or(1);
    let day = u8::try_from(d.day()).unwrap_or(1);
    let Some(date) = Date::new(year, month, day).or_else(|| Date::new(1970, 1, 1)) else {
        unreachable!("1970-01-01 is a valid date")
    };
    debug_assert!(date.year() >= 1970, "the clock is past the epoch");
    debug_assert!(date.month() >= 1 && date.day() >= 1, "calendar-valid");
    date
}

/// A ULID: 48 bits of Unix milliseconds, then 80 random bits. https://github.com/ulid/spec
pub fn new_ulid() -> io::Result<Ulid> {
    let ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(io::Error::other)?
        .as_millis();
    let mut random = [0u8; 10];
    // getrandom's Error is not std::error::Error without the `std` feature; carry its text.
    getrandom::fill(&mut random).map_err(|e| io::Error::other(e.to_string()))?;
    let mut bits: u128 = 0;
    for byte in random {
        bits = (bits << 8) | u128::from(byte);
    }
    debug_assert!(bits < (1u128 << 80), "80 random bits");
    debug_assert!(ms < (1u128 << 48), "48-bit timestamp until year 10889");
    Ok(Ulid::from_u128(((ms & ((1u128 << 48) - 1)) << 80) | bits))
}
