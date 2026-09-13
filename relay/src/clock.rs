//! The one thing the relay takes from the outside world besides HTTP requests: the current wall
//! clock, read only at the edge (`http` handlers) so `store`/`retention` logic takes an explicit
//! `now_ms` and stays deterministic under test — same "clock enters at main/edge" idiom as
//! `txtodo-cli`'s and `txtodo-daemon`'s own `clock.rs`.

use std::time::{SystemTime, UNIX_EPOCH};

/// Milliseconds since the Unix epoch, clamped to 0 on a clock before 1970 rather than panicking.
/// <https://doc.rust-lang.org/std/time/struct.SystemTime.html#method.now>
pub fn now_ms() -> i64 {
    let dur = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    i64::try_from(dur.as_millis()).unwrap_or(i64::MAX)
}
