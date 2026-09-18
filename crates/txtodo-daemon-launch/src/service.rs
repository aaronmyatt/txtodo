//! launchd/systemd render+install+start/stop logic (task `daemon-always-available`, item 2),
//! moved here out of `crates/txtodo-cli/src/commands/service.rs` (which now calls into this
//! module and keeps only its own CLI-specific glue: argument parsing, `println!`, `CliError`
//! conversion). Nothing here prints — this is a library, and `ensure_daemon`'s own best-effort
//! install/start call (`spawn.rs`) must stay silent on the hot path.

use std::path::{Path, PathBuf};
use std::process::Command;

/// The launchd agent template (`deploy/launchd/`).
pub const LAUNCHD_TEMPLATE: &str = include_str!("../../../deploy/launchd/com.txtodo.txtodod.plist");
/// The systemd user unit template (`deploy/systemd/`).
pub const SYSTEMD_TEMPLATE: &str = include_str!("../../../deploy/systemd/txtodod.service");
/// The one global unit's label (ADR 0025: one `txtodod` per device, not one per workspace).
pub const LABEL: &str = "com.txtodo.txtodod";

/// One rendered service: label, file location and contents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rendered {
    /// launchd label / systemd unit name.
    pub label: String,
    /// Where `install` writes it.
    pub path: PathBuf,
    /// The file body.
    pub body: String,
}

/// What [`install`] did, for a caller (`txtodo daemon install`'s own `println!`s) to report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallOutcome {
    /// Labels of any pre-M11 per-workspace units [`install`] found and removed.
    pub migrated: Vec<String>,
    /// Where the new global unit file was written.
    pub path: PathBuf,
}

/// Everything that can go wrong rendering, installing or controlling the service.
#[derive(Debug)]
pub enum ServiceError {
    /// A filesystem operation failed.
    Io(std::io::Error),
    /// A human-readable failure with no underlying `io::Error` (e.g. `$HOME` unset, `launchctl`
    /// exited non-zero, the unit file already exists without `--force`).
    Message(String),
}

impl std::fmt::Display for ServiceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ServiceError::Io(e) => write!(f, "{e}"),
            ServiceError::Message(m) => write!(f, "{m}"),
        }
    }
}

impl std::error::Error for ServiceError {}

/// Where a per-platform service integration keeps its unit files, and the extension its own
/// template uses — the two things [`render`]/[`migrate_old_units`] both need per platform.
fn service_dir_and_ext(home: &Path) -> Option<(PathBuf, &'static str)> {
    if cfg!(target_os = "macos") {
        return Some((home.join("Library").join("LaunchAgents"), "plist"));
    }
    if cfg!(target_os = "linux") {
        return Some((home.join(".config").join("systemd").join("user"), "service"));
    }
    None
}

/// Fills the placeholders of a template.
pub fn render_template(template: &str, label: &str, txtodod: &Path, log_dir: &Path) -> String {
    let out = template
        .replace("{{LABEL}}", label)
        .replace("{{TXTODOD}}", &txtodod.to_string_lossy())
        .replace("{{LOGDIR}}", &log_dir.to_string_lossy());
    debug_assert!(!out.contains("{{"), "every placeholder is filled");
    out
}

/// The one global service file for this platform (ADR 0025), or `None` where txtodo has no
/// service integration yet.
pub fn render(home: &Path, txtodod: &Path) -> Option<Rendered> {
    let (dir, ext) = service_dir_and_ext(home)?;
    // Only the launchd template actually uses {{LOGDIR}}: systemd's own default sends
    // stdout/stderr to the user journal instead.
    let (template, log_dir) = if ext == "plist" {
        (
            LAUNCHD_TEMPLATE,
            home.join("Library").join("Logs").join("txtodo"),
        )
    } else {
        (SYSTEMD_TEMPLATE, PathBuf::new())
    };
    Some(Rendered {
        body: render_template(template, LABEL, txtodod, &log_dir),
        label: LABEL.to_owned(),
        path: dir.join(format!("{LABEL}.{ext}")),
    })
}

/// A pre-M11 per-workspace unit's label, parsed from its file name: `{LABEL}.<8 hex>.<ext>`,
/// distinct from the new global unit's own bare `{LABEL}.<ext>` (no hash segment).
fn old_workspace_label(file_name: &str, ext: &str) -> Option<String> {
    let rest = file_name
        .strip_prefix(LABEL)?
        .strip_prefix('.')?
        .strip_suffix(&format!(".{ext}"))?;
    (rest.len() == 8 && rest.bytes().all(|b| b.is_ascii_hexdigit()))
        .then(|| format!("{LABEL}.{rest}"))
}

/// Finds and removes any pre-M11 per-workspace units (ADR 0025's migration invariant: `install`
/// must never leave a stale per-workspace daemon running alongside the new global one). Best
/// effort per unit — one already stopped, or one `stop_by_label` fails against for any other
/// reason, still has its file removed and its label reported, never fatal to the install itself.
pub fn migrate_old_units(home: &Path) -> Vec<String> {
    let Some((dir, ext)) = service_dir_and_ext(home) else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut migrated = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(label) = old_workspace_label(&name.to_string_lossy(), ext) else {
            continue;
        };
        let _ = stop_by_label(&label);
        if std::fs::remove_file(entry.path()).is_ok() {
            migrated.push(label);
        }
    }
    migrated
}

/// The daemon binary path recorded in an already-rendered unit's `body` — the inverse of
/// `render_template`'s `{{TXTODOD}}` substitution (task `daemon-stale-service-repair`). `None`
/// means the body doesn't look like either template (e.g. a human hand-edited it into something
/// unrecognizable) — treated as "not stale" by [`is_stale`], never as a false positive.
fn installed_program_path(body: &str) -> Option<PathBuf> {
    if let Some((_, rest)) = body.split_once("ExecStart=") {
        return Some(PathBuf::from(rest.lines().next()?.trim()));
    }
    let mut lines = body.lines();
    lines.find(|l| l.contains("<key>ProgramArguments</key>"))?;
    for line in lines {
        match line.trim() {
            "<array>" => continue,
            other => {
                return other
                    .strip_prefix("<string>")
                    .and_then(|s| s.strip_suffix("</string>"))
                    .map(PathBuf::from);
            }
        }
    }
    None
}

/// Whether the unit already installed at `r.path` is stale: it exists on disk, but the binary
/// path it records no longer exists (task `daemon-stale-service-repair`) — e.g. a debug binary
/// inside a git worktree that has since been deleted. `KeepAlive`/`Restart=on-failure` can retry
/// an exec against a missing binary forever without ever succeeding, and nothing short of
/// reinstalling the unit file itself can fix that.
///
/// Deliberately narrow (see `tasks/daemon-stale-service-repair/notes.md`'s design notes): only
/// "the recorded binary is gone" counts as stale. A unit whose binary exists but differs from
/// what would be rendered today is left alone — that may be a human's deliberate customization,
/// not a bug to silently overwrite. No unit installed at all is "not installed", not "stale".
pub fn is_stale(r: &Rendered) -> bool {
    let Ok(body) = std::fs::read_to_string(&r.path) else {
        return false;
    };
    match installed_program_path(&body) {
        Some(path) => !path.is_file(),
        None => false,
    }
}

/// `$HOME` (or `%USERPROFILE%`), or an error if neither is set.
pub fn home_dir() -> Result<PathBuf, ServiceError> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .ok_or_else(|| ServiceError::Message("txtodo daemon: $HOME is not set".into()))
}

fn run_ctl(program: &str, args: &[&str]) -> Result<(), ServiceError> {
    let status = Command::new(program)
        .args(args)
        .status()
        .map_err(ServiceError::Io)?;
    if status.success() {
        return Ok(());
    }
    Err(ServiceError::Message(format!(
        "txtodo daemon: `{program} {}` exited with {status}",
        args.join(" ")
    )))
}

fn uid() -> String {
    // `id -u` is POSIX; launchctl needs the gui/<uid> domain.
    Command::new("id")
        .arg("-u")
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
        .unwrap_or_else(|| "501".to_owned())
}

/// Migrates any pre-M11 per-workspace units, then writes the one global unit file. Idempotent
/// failure mode: an existing file without `force` is a [`ServiceError`], which every caller of
/// this (including `ensure_daemon`'s best-effort install) treats as "already installed", not a
/// real problem.
pub fn install(home: &Path, r: &Rendered, force: bool) -> Result<InstallOutcome, ServiceError> {
    if r.path.exists() && !force {
        return Err(ServiceError::Message(format!(
            "txtodo daemon: {} exists; use --force to overwrite",
            r.path.display()
        )));
    }
    let migrated = migrate_old_units(home);
    if let Some(dir) = r.path.parent() {
        std::fs::create_dir_all(dir).map_err(ServiceError::Io)?;
    }
    std::fs::write(&r.path, &r.body).map_err(ServiceError::Io)?;
    Ok(InstallOutcome {
        migrated,
        path: r.path.clone(),
    })
}

/// Loads and starts the service (launchd `bootstrap`+`kickstart -k`, or systemd `enable --now`).
pub fn start(r: &Rendered) -> Result<(), ServiceError> {
    if !r.path.exists() {
        return Err(ServiceError::Message(format!(
            "txtodo daemon: {} missing; run `txtodo daemon install`",
            r.path.display()
        )));
    }
    if cfg!(target_os = "macos") {
        // https://keith.github.io/xcode-man-pages/launchctl.1.html — bootstrap loads, kickstart runs now
        let domain = format!("gui/{}", uid());
        run_ctl(
            "launchctl",
            &["bootstrap", &domain, &r.path.to_string_lossy()],
        )?;
        run_ctl(
            "launchctl",
            &["kickstart", "-k", &format!("{domain}/{}", r.label)],
        )
    } else {
        run_ctl("systemctl", &["--user", "daemon-reload"])?;
        run_ctl(
            "systemctl",
            &["--user", "enable", "--now", &format!("{}.service", r.label)],
        )
    }
}

/// Stops and unloads the service.
pub fn stop(r: &Rendered) -> Result<(), ServiceError> {
    stop_by_label(&r.label)
}

fn stop_by_label(label: &str) -> Result<(), ServiceError> {
    if cfg!(target_os = "macos") {
        run_ctl("launchctl", &["bootout", &format!("gui/{}/{label}", uid())])
    } else {
        run_ctl(
            "systemctl",
            &["--user", "disable", "--now", &format!("{label}.service")],
        )
    }
}

#[cfg(test)]
#[path = "service_tests.rs"]
mod tests;
