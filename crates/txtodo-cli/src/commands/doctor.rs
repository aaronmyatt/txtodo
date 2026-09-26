//! `txtodo doctor` (plan M3, design §5; `keystore` added plan M4 `sync-keystore`, `transport`
//! added plan M4 `sync-lan-transport`): socket, watcher, files, clock, config, keystore,
//! transport. Seven fixed checks in a fixed order so scripts can index them, plus one row per
//! known sync peer (plan M4 tasks/model-hlc-skew-guard) and one per pair of registered workspaces
//! that share lists (`doctor_overlap.rs`) — each carries the command that fixes it.
//! Exit status 1 when any check fails. `--verbose` tails the daemon's JSON log when one exists.

use crate::client::{self, Mode, SOCKET_REL};
use crate::commands::doctor_clock::{clock_check, config_check};
use crate::commands::doctor_overlap::{overlap_checks, registered_workspaces};
use crate::commands::doctor_transport::{offers_check, transport_check};
use crate::commands::doctor_version::version_check;
use crate::{CliError, Ctx, json};
use std::path::Path;
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

/// Everything the connected daemon (if any) contributed: the fixed checks, `Health` (feeds
/// `clock`/`keystore`/`transport`), known sync peers, and the live connection itself — kept open
/// so `other_workspace_checks` can probe the rest of the registry without reconnecting.
struct DaemonState {
    checks: Vec<Check>,
    health: Option<pb::HealthResponse>,
    devices: Vec<pb::Device>,
    daemon: Option<Box<client::Daemon>>,
}

/// The daemon side: socket reachability and, through Health, the watcher; when connected, also
/// the known sync peers `DeviceList` reports (best-effort — a failed list still leaves the health
/// checks meaningful, so it degrades to an empty peer set rather than failing the whole command).
fn daemon_checks(ctx: &Ctx) -> DaemonState {
    let socket = ctx.paths.dir.join(SOCKET_REL);
    let unknown = check("watcher", Status::Warn, "unknown: no daemon");
    let env = crate::config::Env::from_process().unwrap_or_default();
    match client::select(&ctx.paths.dir, false, &env) {
        Ok(Mode::Direct) => {
            let fix = format!(
                "no socket at {}; run `txtodo daemon start`",
                socket.display()
            );
            DaemonState {
                checks: vec![check("socket", Status::Fail, fix), unknown],
                health: None,
                devices: Vec::new(),
                daemon: None,
            }
        }
        Err(e) => DaemonState {
            checks: vec![check("socket", Status::Fail, e.to_string()), unknown],
            health: None,
            devices: Vec::new(),
            daemon: None,
        },
        Ok(Mode::Daemon(mut d)) => {
            let (checks, health) = health_checks(&mut d);
            let devices = if health.is_some() {
                d.device_list().unwrap_or_default()
            } else {
                Vec::new()
            };
            DaemonState {
                checks,
                health,
                devices,
                daemon: Some(d),
            }
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

/// Advisory only, never a FAIL: nudges toward `txtodo skill install` (agent playbook) when nothing
/// has installed it yet.
fn skill_check() -> Check {
    if super::skill::claude_installed() {
        check("skill", Status::Ok, "agent playbook installed")
    } else {
        check(
            "skill",
            Status::Warn,
            "no agent playbook installed; run `txtodo skill install` so agents can work this \
             backlog on their own",
        )
    }
}

/// Opens todo.txt for append without writing.
fn files_check(ctx: &Ctx) -> Check {
    let mut problems = Vec::new();
    for (name, path) in [(ctx.paths.todo_file.as_str(), &ctx.paths.todo)] {
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

/// The resolved sync-keystore backend (plan M4 `sync-keystore`), by name, straight from Health —
/// "a human should be able to answer where are my keys? without reading code." `"memory"` (task
/// `relay-id-keystore`) means no persisted relay identity: a FAIL, since the daemon only logs it
/// once, at startup, and a stale relay allowlist entry is the invisible cost later.
fn keystore_check(health: Option<&pb::HealthResponse>) -> Check {
    match health {
        Some(h) if h.key_store_backend == "memory" => check(
            "keystore",
            Status::Fail,
            "backend: memory; the device's static, signing and group keys and its relay \
             identity are reminted at every start, so relay and LAN sync are off for this run \
             and every paired peer must re-pair once keys persist; fix the OS keychain \
             (--key-store auto/os) or pass --key-store file",
        ),
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
            let name = if d.name.is_empty() {
                d.id.clone()
            } else {
                format!("{} ({})", d.name, d.id)
            };
            // Task default-workspace-pairing-consent: the default merges only with an own device.
            let label = if d.own_device {
                name
            } else {
                format!("{name} [not own: default kept apart]")
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

/// One line per *other* registered workspace (ADR 0025, task `cli-doctor-multi-workspace`) — the
/// seven fixed checks above already cover the current one in full depth, so this stays a cheap
/// per-entry `Health` probe, not a second full battery of checks. Best-effort: a legacy
/// `--dir`-bridge daemon has no registry, so an empty `workspaces`
/// (`doctor_overlap::registered_workspaces`) just means there is nothing more to report, not a
/// doctor failure — `run`'s existing seven checks already told the human that story if it matters.
fn other_workspace_checks(
    daemon: Option<&mut client::Daemon>,
    workspaces: &[pb::WorkspaceInfo],
    current: &Path,
) -> Vec<Check> {
    let Some(daemon) = daemon else {
        return Vec::new();
    };
    let current = current
        .canonicalize()
        .unwrap_or_else(|_| current.to_owned());
    workspaces
        .iter()
        .filter(|w| Path::new(&w.root) != current)
        .map(|w| {
            let label = format!("{}  {}", w.workspace_id, w.root);
            match daemon.health_for_id(&w.workspace_id) {
                Ok(h) => check(
                    "workspace",
                    Status::Ok,
                    format!("{label}: {} document(s)", h.documents),
                ),
                Err(e) => check("workspace", Status::Warn, format!("{label}: {e}")),
            }
        })
        .collect()
}

/// Runs every check, prints the report, exits 1 on any failure.
pub fn run(ctx: &Ctx, verbose: bool) -> Result<(), CliError> {
    let mut state = daemon_checks(ctx);
    let mut checks = std::mem::take(&mut state.checks);
    let health = state.health.take();
    let devices = std::mem::take(&mut state.devices);
    checks.push(files_check(ctx));
    checks.push(clock_check(health.as_ref()));
    checks.push(config_check(ctx));
    checks.push(keystore_check(health.as_ref()));
    checks.push(transport_check(health.as_ref()));
    debug_assert_eq!(checks.len(), 7, "seven fixed checks in a fixed order");
    // After the fixed seven, so their order and count stay what scripts already read.
    checks.push(version_check(health.as_ref()));
    checks.push(offers_check(health.as_ref()));
    checks.extend(peer_checks(&devices));
    let workspaces = registered_workspaces(state.daemon.as_deref_mut());
    checks.extend(other_workspace_checks(
        state.daemon.as_deref_mut(),
        &workspaces,
        &ctx.paths.dir,
    ));
    checks.extend(overlap_checks(&workspaces));
    checks.push(skill_check());
    checks.extend(super::layout::doctor_checks(
        ctx,
        state.daemon.as_deref_mut(),
    ));
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
        // Which build is answering, before what it found (task version-info).
        println!("txtodo {}", crate::buildinfo::VERSION_LINE);
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

#[cfg(test)]
#[path = "doctor_tests.rs"]
mod tests;
