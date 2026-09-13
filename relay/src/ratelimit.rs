//! Per-group request cap (tasks/relay-reference/notes.md: "a per-group request cap so the relay
//! isn't free scratch space") — a fixed-window counter. This guards request *rate*, separate
//! from `store`'s per-device blob/wake-queue caps, which guard storage.

use std::collections::HashMap;

/// One group's request count within its current fixed window.
struct Window {
    started_at_ms: i64,
    count: u32,
}

/// Per-group fixed-window rate limiter guarding the HTTP surface (`http::AppState`). One entry
/// per group id ever seen — bounded in practice by how many distinct groups actually talk to
/// this relay, the same assumption `store`'s per-`(group, device)` rows already make.
#[derive(Default)]
pub struct RateLimiter {
    windows: HashMap<String, Window>,
}

impl RateLimiter {
    /// True if `group` may make one more request at `now_ms` under `cap` requests per
    /// `window_ms`; records the request when it does. A new window starts the first time a
    /// group is seen after its previous window has elapsed.
    pub fn allow(&mut self, group: &str, now_ms: i64, cap: u32, window_ms: i64) -> bool {
        let window = self.windows.entry(group.to_owned()).or_insert(Window {
            started_at_ms: now_ms,
            count: 0,
        });
        if now_ms.saturating_sub(window.started_at_ms) >= window_ms {
            window.started_at_ms = now_ms;
            window.count = 0;
        }
        if window.count >= cap {
            return false;
        }
        window.count += 1;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn caps_requests_per_window_then_resets() {
        let mut limiter = RateLimiter::default();
        for _ in 0..3 {
            assert!(limiter.allow("g1", 0, 3, 1000));
        }
        assert!(
            !limiter.allow("g1", 500, 3, 1000),
            "fourth request in the same window is refused"
        );
        assert!(
            limiter.allow("g1", 1500, 3, 1000),
            "a new window resets the count"
        );
        assert!(
            limiter.allow("g2", 500, 3, 1000),
            "a different group has its own window"
        );
    }
}
