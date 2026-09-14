//! `txtodo doctor` (plan M3, design §5; `keystore` added plan M4 `sync-keystore`, `transport`
//! added plan M4 `sync-lan-transport`): socket, watcher, files, clock, config, keystore,
//! transport. Seven fixed checks in a fixed order so scripts can index them, plus one row per
//! known sync peer (plan M4 tasks/model-hlc-skew-guard) — each carries the command that fixes it.
//! Exit status 1 when any check fails. `--verbose` tails the daemon's JSON log when one exists.

use crate::client::{self, Mode, SOCKET_REL};
use crate::commands::doctor_transport::transport_check;
use crate::{CliError, Ctx, json};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};
use txtodo_proto::v1 as pb;

/// Most log lines `--verbose` prints.
pub const VERBOSE_LOG_LINES: usize = 100;
/// Where the daemon writes JSON logs, relative to the todo dir.
pub const LOGS_REL: &str = ".txtodo/logs";

/// One check's verdict.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// Fine.
    Ok,
    /// Works, but worth a look.
    Warn,
    /// Broken; the detail says how to fix it.
    Fail,
}

impl Status {
    fn label(self) -> &'static str {
        match self {
            Status::Ok => "ok",
            Status::Warn => "warn",
            Status::Fail => "FAIL",
        }
    }
}

/// One row of the report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Check {
    /// Fixed name: socket, watcher, files, clock, config.
    pub name: &'static str,
    /// Verdict.
    pub status: Status,
    /// What was seen and, on failure, what fixes it.
    pub detail: String,
}

pub(super) fn check(name: &'static str, status: Status, detail: impl Into<String>) -> Check {
    Check {
        name,
        status,
        detail: detail.into(),
    }
}

/// The daemon side: socket reachability and, through Health, the watcher; when connected, also
/// the known sync peers `DeviceList` reports (best-effort — a failed list still leaves the health
/// checks meaningful, so it degrades to an empty peer set rather than failing the whole command).
fn daemon_checks(ctx: &Ctx) -> (Vec<Check>, Option<pb::HealthResponse>, Vec<pb::Device>) {
    let socket = ctx.paths.dir.join(SOCKET_REL);
    let unknown = check("watcher", Status::Warn, "unknown: no daemon");
    let env = crate::config::Env::from_process().unwrap_or_default();
    match client::select(&ctx.paths.dir, false, &env) {
        Ok(Mode::Direct) => {
            let fix = format!(
                "no socket at {}; run `txtodo daemon start`",
                socket.display()
            );
            (
                vec![check("socket", Status::Fail, fix), unknown],
                None,
                Vec::new(),
            )
        }
        Err(e) => (
            vec![check("socket", Status::Fail, e.to_string()), unknown],
            None,
            Vec::new(),
        ),
        Ok(Mode::Daemon(mut d)) => {
            let (checks, health) = health_checks(&mut d);
            let devices = if health.is_some() {
                d.device_list().unwrap_or_default()
            } else {
                Vec::new()
            };
            (checks, health, devices)
        }
    }
}

/// With a connection: Health tells us the version, document count and watcher liveness.
fn health_checks(d: &mut client::Daemon) -> (Vec<Check>, Option<pb::HealthResponse>) {
    let h = match d.health() {
        Ok(h) => h,
        Err(e) => {
            return (
                vec![
                    check("socket", Status::Fail, e.to_string()),
                    check("watcher", Status::Warn, "unknown"),
                ],
                None,
            );
        }
    };
    let socket_ok = check(
        "socket",
        Status::Ok,
        format!("txtodod {} answers, {} document(s)", h.version, h.documents),
    );
    let watcher = if h.watcher_alive {
        check(
            "watcher",
            Status::Ok,
            format!("alive; last event {} ms ago", h.last_event_age_ms),
        )
    } else {
        check(
            "watcher",
            Status::Fail,
            "not running; restart with `txtodo daemon stop && txtodo daemon start`",
        )
    };
    debug_assert_eq!(socket_ok.name, "socket");
    (vec![socket_ok, watcher], Some(h))
}

/// Opens todo.txt for append without writing.
fn files_check(ctx: &Ctx) -> Check {
    let mut problems = Vec::new();
    for (name, path) in [("todo.txt", &ctx.paths.todo)] {
        if !path.exists() {
            problems.push(format!("{name} missing (created on first add)"));
            continue;
        }
        if let Err(e) = std::fs::OpenOptions::new().append(true).open(path) {
            return check(
                "files",
                Status::Fail,
                format!(
                    "{} is not writable: {e}; fix with chmod u+w",
                    path.display()
                ),
            );
        }
    }
    if problems.is_empty() {
        check(
            "files",
            Status::Ok,
            format!("{} writable", ctx.paths.dir.display()),
        )
    } else {
        check("files", Status::Warn, problems.join("; "))
    }
}

/// The wall clock must not be behind the newest op we know of.
fn clock_check(health: Option<&pb::HealthResponse>) -> Check {
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

fn config_check(ctx: &Ctx) -> Check {
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

/// The resolved sync-keystore backend (plan M4 `sync-keystore`), by name, straight from Health —
/// "a human should be able to answer where are my keys? without reading code."
fn keystore_check(health: Option<&pb::HealthResponse>) -> Check {
    match health {
        Some(h) if !h.key_store_backend.is_empty() => check(
            "keystore",
            Status::Ok,
            format!("backend: {}", h.key_store_backend),
        ),
        Some(_) => check("keystore", Status::Warn, "daemon did not report a backend"),
        None => check("keystore", Status::Warn, "unknown: no daemon"),
    }
}

/// One line per known, active sync peer (plan M4 `tasks/model-hlc-skew-guard`): a peer behind is
/// safe (warn); a peer ahead would be refused by a real sync session (fail); no sample yet is a
/// warn, never a guessed verdict. Removed devices and this device itself are not peers to report.
fn peer_checks(devices: &[pb::Device]) -> Vec<Check> {
    devices
        .iter()
        .filter(|d| !d.is_self && !d.removed)
        .map(|d| {
            let label = if d.name.is_empty() {
                d.id.clone()
            } else {
                format!("{} ({})", d.name, d.id)
            };
            let (status, detail) = match pb::SkewStatus::try_from(d.skew_status)
                .unwrap_or(pb::SkewStatus::Unspecified)
            {
                pb::SkewStatus::Ok => (Status::Ok, format!("{label}: clock ok")),
                pb::SkewStatus::Behind => (
                    Status::Warn,
                    format!("{label}: clock behind by {} ms; check NTP on that device", d.skew_ms),
                ),
                pb::SkewStatus::Ahead => (
                    Status::Fail,
                    format!(
                        "{label}: clock ahead by {} ms; a sync session with it is refused until fixed",
                        d.skew_ms
                    ),
                ),
                pb::SkewStatus::Unknown | pb::SkewStatus::Unspecified => {
                    (Status::Warn, format!("{label}: no clock sample yet"))
                }
            };
            check("peer", status, detail)
        })
        .collect()
}

/// Runs every check, prints the report, exits 1 on any failure.
pub fn run(ctx: &Ctx, verbose: bool) -> Result<(), CliError> {
    let (mut checks, health, devices) = daemon_checks(ctx);
    checks.push(files_check(ctx));
    checks.push(clock_check(health.as_ref()));
    checks.push(config_check(ctx));
    checks.push(keystore_check(health.as_ref()));
    checks.push(transport_check(health.as_ref()));
    debug_assert_eq!(checks.len(), 7, "seven fixed checks in a fixed order");
    checks.extend(peer_checks(&devices));
    if ctx.json {
        let rows: Vec<String> = checks
            .iter()
            .map(|c| {
                format!(
                    r#"{{"name":{},"status":{},"detail":{}}}"#,
                    json::str(c.name),
                    json::str(c.status.label()),
                    json::str(&c.detail)
                )
            })
            .collect();
        println!("[{}]", rows.join(","));
    } else {
        for c in &checks {
            println!("{:<8} {:<5} {}", c.name, c.status.label(), c.detail);
        }
    }
    if verbose {
        print_recent_log(&ctx.paths.dir.join(LOGS_REL));
    }
    if checks.iter().any(|c| c.status == Status::Fail) {
        return Err(CliError::Reported);
    }
    Ok(())
}

/// The last `VERBOSE_LOG_LINES` lines of the newest log file, if the daemon has written any.
fn print_recent_log(logs: &Path) {
    let newest = std::fs::read_dir(logs)
        .ok()
        .and_then(|rd| rd.flatten().map(|e| e.path()).filter(|p| p.is_file()).max());
    let Some(path) = newest else {
        println!("-- no daemon logs under {}", logs.display());
        return;
    };
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    let lines: Vec<&str> = text.lines().collect();
    let start = lines.len().saturating_sub(VERBOSE_LOG_LINES);
    println!(
        "-- {} (last {} of {} lines)",
        path.display(),
        lines.len() - start,
        lines.len()
    );
    for line in &lines[start..] {
        println!("{line}");
    }
}
