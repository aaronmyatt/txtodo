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
mod tests {
    use super::*;

    #[test]
    fn render_is_one_global_unit_with_no_workspace_argument() {
        let body = render_template(
            LAUNCHD_TEMPLATE,
            LABEL,
            Path::new("/usr/local/bin/txtodod"),
            Path::new("/home/u/Library/Logs/txtodo"),
        );
        assert!(body.contains("<string>/usr/local/bin/txtodod</string>"));
        assert!(body.contains("<string>/home/u/Library/Logs/txtodo/launchd.out.log</string>"));
        assert!(
            !body.contains("<string>--dir</string>"),
            "true global mode takes no --dir argument"
        );
        assert!(!body.contains("{{"));
        let unit = render_template(
            SYSTEMD_TEMPLATE,
            LABEL,
            Path::new("/bin/txtodod"),
            Path::new("/home/u/Library/Logs/txtodo"),
        );
        assert_eq!(
            unit.lines().find(|l| l.starts_with("ExecStart=")),
            Some("ExecStart=/bin/txtodod")
        );
        let r = render(Path::new("/home/u"), Path::new("/bin/txtodod"));
        if cfg!(any(target_os = "macos", target_os = "linux")) {
            let r = r.unwrap_or_else(|| panic!("supported platform"));
            assert_eq!(r.label, LABEL);
            assert!(r.path.starts_with("/home/u"));
            assert_eq!(
                r.path.file_stem().and_then(|s| s.to_str()),
                Some(LABEL),
                "one bare label, no per-workspace hash suffix: {}",
                r.path.display()
            );
        }
    }

    #[test]
    fn old_workspace_label_recognizes_the_pre_m11_hash_suffix_only() {
        assert_eq!(
            old_workspace_label("com.txtodo.txtodod.1a2b3c4d.plist", "plist"),
            Some("com.txtodo.txtodod.1a2b3c4d".to_owned())
        );
        assert_eq!(
            old_workspace_label("com.txtodo.txtodod.1a2b3c4d.service", "service"),
            Some("com.txtodo.txtodod.1a2b3c4d".to_owned())
        );
        // The new global unit's own file must never be mistaken for an old one to migrate.
        assert_eq!(
            old_workspace_label("com.txtodo.txtodod.plist", "plist"),
            None
        );
        assert_eq!(old_workspace_label("not-ours.plist", "plist"), None);
        assert_eq!(
            old_workspace_label("com.txtodo.txtodod.notquite8x.plist", "plist"),
            None,
            "wrong-length suffix is not a recognized old label"
        );
    }

    #[test]
    fn migrate_old_units_removes_matching_files_and_leaves_the_new_one_alone() {
        let home = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
        let (dir, ext) = service_dir_and_ext(home.path())
            .unwrap_or_else(|| panic!("supported platform for this test"));
        std::fs::create_dir_all(&dir).unwrap_or_else(|e| panic!("mkdir: {e}"));
        let old = dir.join(format!("{LABEL}.deadbeef.{ext}"));
        let new = dir.join(format!("{LABEL}.{ext}"));
        std::fs::write(&old, "old").unwrap_or_else(|e| panic!("write: {e}"));
        std::fs::write(&new, "new").unwrap_or_else(|e| panic!("write: {e}"));

        let migrated = migrate_old_units(home.path());
        assert_eq!(migrated, vec![format!("{LABEL}.deadbeef")]);
        assert!(!old.exists(), "the old per-workspace unit is removed");
        assert!(new.exists(), "the new global unit's own file is untouched");
    }

    #[test]
    fn install_reports_migrated_units_and_the_written_path() {
        let home = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
        let r = render(home.path(), Path::new("/bin/txtodod"))
            .unwrap_or_else(|| panic!("supported platform for this test"));
        let outcome = install(home.path(), &r, false).unwrap_or_else(|e| panic!("install: {e}"));
        assert_eq!(outcome.path, r.path);
        assert!(outcome.migrated.is_empty(), "nothing to migrate yet");
        assert!(r.path.exists());

        let err = install(home.path(), &r, false)
            .expect_err("a second install without --force must refuse to clobber");
        assert!(err.to_string().contains("--force"));

        let forced =
            install(home.path(), &r, true).unwrap_or_else(|e| panic!("forced install: {e}"));
        assert_eq!(forced.path, r.path);
    }
}
