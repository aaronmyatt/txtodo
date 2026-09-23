//! `clock_check` and `config_check`, split out of `doctor.rs` purely for its file-length budget,
//! the same pattern as `doctor_transport.rs`.

use super::doctor::{Check, Status, check};
use crate::Ctx;
use std::time::{SystemTime, UNIX_EPOCH};
use txtodo_proto::v1 as pb;

/// The wall clock must not be behind the newest op we know of.
pub(super) fn clock_check(health: Option<&pb::HealthResponse>) -> Check {
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let now_ms = u64::try_from(now_ms).unwrap_or(u64::MAX);
    match health {
        Some(h) if now_ms < h.started_at_ms => check(
            "clock",
            Status::Warn,
            format!(
                "system clock ({now_ms} ms) is behind the daemon start ({} ms); check NTP",
                h.started_at_ms
            ),
        ),
        _ => check(
            "clock",
            Status::Ok,
            format!("system clock {now_ms} ms since the epoch"),
        ),
    }
}

pub(super) fn config_check(ctx: &Ctx) -> Check {
    if ctx.paths.config.exists() {
        check("config", Status::Ok, ctx.paths.config.display().to_string())
    } else {
        check(
            "config",
            Status::Ok,
            format!("{} (missing, defaults apply)", ctx.paths.config.display()),
        )
    }
}
