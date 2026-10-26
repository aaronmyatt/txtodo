//! Today's local calendar date, the one date the daemon writes into a file on its own: a client's
//! new line's creation date (ADR 0011: the daemon's local date at write time, `YYYY-MM-DD`, no
//! time zone in the file). Read at the wire, once per `Apply`, and passed down as a value.
//! Ref: <https://docs.rs/jiff/0.2/jiff/struct.Zoned.html#method.now>

use txtodo_core::Date;

/// Today in the system's local time zone.
pub fn today_local() -> Date {
    let d = jiff::Zoned::now().date();
    let year = u16::try_from(d.year()).unwrap_or(1970);
    let month = u8::try_from(d.month()).unwrap_or(1);
    let day = u8::try_from(d.day()).unwrap_or(1);
    Date::new(year, month, day)
        .or_else(|| Date::new(1970, 1, 1))
        .unwrap_or_else(|| unreachable!("1970-01-01 is a valid date"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn today_is_a_real_date_past_the_epoch() {
        assert!(today_local().year() >= 2026);
    }
}
