//! launchd/systemd render+install+start/stop logic (task `daemon-always-available`, item 2),
//! moved here out of `crates/txtodo-cli/src/commands/service.rs` (which now calls into this
//! module and keeps only its own CLI-specific glue: argument parsing, `println!`, `CliError`
//! conversion). Nothing here prints — this is a library, and `ensure_daemon`'s own best-effort
//! install/start call (`spawn.rs`) must stay silent on the hot path.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

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
    // A real `ExecStart=` directive only: line-anchored, so a comment or `ExecStartPre=` never
    // matches. systemd splits the value on whitespace and allows `@`, `-`, `:`, `+`, `!` prefixes
    // before the executable, so the binary is the first token with those stripped — a
    // human-added argument or prefix must not read as "binary gone".
    // Ref: https://www.freedesktop.org/software/systemd/man/latest/systemd.service.html#Command%20lines
    if let Some(rest) = body
        .lines()
        .find_map(|l| l.trim_start().strip_prefix("ExecStart="))
    {
        let program = rest
            .trim_start()
            .trim_start_matches(['@', '-', ':', '+', '!'])
            .split_whitespace()
            .next()?;
        return Some(PathBuf::from(program));
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
/// path it records no longer exists (task `daemon-stale-service-repair`), or it still carries the
/// old unconditional `KeepAlive` (`has_unconditional_keepalive`) — e.g. a debug binary
/// inside a git worktree that has since been deleted. `KeepAlive`/`Restart=on-failure` can retry
/// an exec against a missing binary forever without ever succeeding, and nothing short of
/// reinstalling the unit file itself can fix that.
///
/// Deliberately narrow (see `tasks/daemon-stale-service-repair/notes.md`'s design notes): only
/// "the recorded binary is gone" and the one known-bad `KeepAlive` shape count as stale. A unit
/// whose binary exists but differs from what would be rendered today is left alone — that may be
/// a human's deliberate customization, not a bug to silently overwrite. No unit installed at all
/// is "not installed", not "stale".
pub fn is_stale(r: &Rendered) -> bool {
    let Ok(body) = std::fs::read_to_string(&r.path) else {
        return false;
    };
    let binary_gone = installed_program_path(&body).is_some_and(|path| !path.is_file());
    binary_gone || has_unconditional_keepalive(&body)
}

/// True for a launchd plist written by a build that predates `KeepAlive.SuccessfulExit=false`:
/// `<key>KeepAlive</key>` followed directly by `<true/>`. That shape respawns a daemon that lost
/// the pid lock forever, so it is repaired like a dead binary path (root todo
/// id:01M2VV1ZXDK24H3P6Z4DJ2P8YM) — a specific known-bad shape, not a human's customization.
fn has_unconditional_keepalive(body: &str) -> bool {
    let mut lines = body.lines().map(str::trim).filter(|l| !l.is_empty());
    lines.any(|l| l == "<key>KeepAlive</key>") && lines.next() == Some("<true/>")
}

/// `$HOME` (or `%USERPROFILE%`), or an error if neither is set.
pub fn home_dir() -> Result<PathBuf, ServiceError> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .ok_or_else(|| ServiceError::Message("txtodo daemon: $HOME is not set".into()))
}

/// The single choke point for every `launchctl`/`systemctl` call in this crate, so the
/// `TXTODO_NO_SERVICE=1` guard ([`crate::service_disabled`]) cannot be missed by a new caller.
/// A refusal is an ordinary `Err`, which every best-effort caller already ignores and an explicit
/// `txtodo daemon start|stop` prints.
fn run_ctl(program: &str, args: &[&str]) -> Result<(), ServiceError> {
    if crate::service_disabled() {
        return Err(ServiceError::Message(format!(
            "txtodo daemon: TXTODO_NO_SERVICE=1 is set; refusing `{program} {}`",
            args.join(" ")
        )));
    }
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

/// Loads and starts the service: launchd `bootstrap` for a unit that is not loaded (its
/// `RunAtLoad` starts it), `kickstart -k` for one that already is; systemd `enable --now`.
///
/// Bootstrap alone, never bootstrap then kickstart: `kickstart -k` SIGTERMs the job `RunAtLoad` has
/// just started and starts it again, so a fresh `txtodo daemon start` showed `runs = 2` and last
/// exit -15, and the daemon's cold open was paid twice (root todo id:01M2WX72DCCZC1AJFDDX1WZ7EE).
pub fn start(r: &Rendered) -> Result<(), ServiceError> {
    if !r.path.exists() {
        return Err(ServiceError::Message(format!(
            "txtodo daemon: {} missing; run `txtodo daemon install`",
            r.path.display()
        )));
    }
    if cfg!(target_os = "macos") {
        let domain = format!("gui/{}", uid());
        let commands =
            launchd_start_commands(&domain, r, launchd_has(&format!("{domain}/{}", r.label)));
        for args in &commands {
            let args: Vec<&str> = args.iter().map(String::as_str).collect();
            run_ctl("launchctl", &args)?;
        }
        Ok(())
    } else {
        run_ctl("systemctl", &["--user", "daemon-reload"])?;
        run_ctl(
            "systemctl",
            &["--user", "enable", "--now", &format!("{}.service", r.label)],
        )
    }
}

/// The `launchctl` invocations `start` runs. https://keith.github.io/xcode-man-pages/launchctl.1.html
fn launchd_start_commands(domain: &str, r: &Rendered, loaded: bool) -> Vec<Vec<String>> {
    if loaded {
        return vec![vec![
            "kickstart".to_owned(),
            "-k".to_owned(),
            format!("{domain}/{}", r.label),
        ]];
    }
    vec![vec![
        "bootstrap".to_owned(),
        domain.to_owned(),
        r.path.to_string_lossy().into_owned(),
    ]]
}

/// Whether the service manager has this unit loaded and running or starting: launchd has the job
/// registered, or systemd reports it `active`/`activating`. False when service control is disabled
/// (`TXTODO_NO_SERVICE=1`) or the manager cannot be asked. `ensure_daemon` waits for a unit that is
/// loaded — it is on its way up — and spawns its own daemon for one that is merely installed, such
/// as after `txtodo daemon stop`, which used to make every client wait out its full timeout.
pub fn is_loaded(r: &Rendered) -> bool {
    if crate::service_disabled() {
        return false;
    }
    if cfg!(target_os = "macos") {
        return launchd_has(&format!("gui/{}/{}", uid(), r.label));
    }
    Command::new("systemctl")
        .args(["--user", "is-active", &format!("{}.service", r.label)])
        .output()
        .is_ok_and(|o| systemd_state_is_loaded(&String::from_utf8_lossy(&o.stdout)))
}

/// `systemctl is-active` prints one word; these two mean the unit is up or on its way up.
/// https://www.freedesktop.org/software/systemd/man/latest/systemctl.html#is-active%20PATTERN%E2%80%A6
fn systemd_state_is_loaded(stdout: &str) -> bool {
    matches!(stdout.trim(), "active" | "activating")
}

/// Stops and unloads the service.
pub fn stop(r: &Rendered) -> Result<(), ServiceError> {
    stop_by_label(&r.label)
}

/// How long [`stop_by_label`] waits for launchd to finish removing a job after `bootout`.
/// `bootout` returns once teardown is *requested*: the job stays registered until its process
/// exits (SIGTERM, then SIGKILL after the plist's `ExitTimeOut`, 20s by default), and a `bootstrap`
/// in that window fails with the unhelpful "5: Input/output error" (launchd's own log says "same
/// label as an existing service"). The daemon's graceful shutdown really does take seconds.
/// https://keith.github.io/xcode-man-pages/launchd.plist.5.html — see `ExitTimeOut`
const BOOTOUT_SETTLE: Duration = Duration::from_secs(30);
const BOOTOUT_POLL: Duration = Duration::from_millis(100);

fn stop_by_label(label: &str) -> Result<(), ServiceError> {
    if cfg!(target_os = "macos") {
        let target = format!("gui/{}/{label}", uid());
        run_ctl("launchctl", &["bootout", &target])?;
        if wait_until(BOOTOUT_SETTLE, BOOTOUT_POLL, || !launchd_has(&target)) {
            return Ok(());
        }
        Err(ServiceError::Message(format!(
            "txtodo daemon: {label} still registered with launchd {}s after `bootout`",
            BOOTOUT_SETTLE.as_secs()
        )))
    } else {
        // `disable --now` blocks until the unit has stopped, so there is nothing to wait for.
        run_ctl(
            "systemctl",
            &["--user", "disable", "--now", &format!("{label}.service")],
        )
    }
}

/// Whether launchd still has `target` (`gui/<uid>/<label>`) registered: `launchctl print` exits
/// non-zero ("Could not find service") once it is gone. A launchctl that cannot even run reads as
/// gone, so a missing binary never turns into a 30s stall.
/// https://keith.github.io/xcode-man-pages/launchctl.1.html
fn launchd_has(target: &str) -> bool {
    Command::new("launchctl")
        .args(["print", target])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// Polls `done` every `poll` until it holds (`true`) or `timeout` passes (`false`).
fn wait_until(timeout: Duration, poll: Duration, mut done: impl FnMut() -> bool) -> bool {
    let start = Instant::now();
    while !done() {
        if start.elapsed() >= timeout {
            return false;
        }
        std::thread::sleep(poll);
    }
    true
}

#[cfg(test)]
#[path = "service_tests.rs"]
mod tests;
