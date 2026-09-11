//! What the CLI takes from the outside world besides files: today's local date. Called only at the
//! edge (`main`'s dispatch); command logic receives values.

use txtodo_core::Date;

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
