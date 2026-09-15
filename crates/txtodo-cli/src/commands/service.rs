//! `txtodo daemon install|start|stop|status` (plan M3; ADR 0025 as of M11 — one global unit per
//! device, not one per workspace): render the platform template from `deploy/` with the daemon
//! binary, hand it to launchd or systemd --user, and probe the socket for status. `install` also
//! migrates (stops and removes) any pre-M11 per-workspace units it finds. Windows is M10.
//! Templates are embedded so the binary is self-contained.

use crate::client::{self, Mode, SOCKET_REL};
use crate::{CliError, Ctx};
use std::path::{Path, PathBuf};
use std::process::Command;

/// The launchd agent template (`deploy/launchd/`).
pub const LAUNCHD_TEMPLATE: &str =
    include_str!("../../../../deploy/launchd/com.txtodo.txtodod.plist");
/// The systemd user unit template (`deploy/systemd/`).
pub const SYSTEMD_TEMPLATE: &str = include_str!("../../../../deploy/systemd/txtodod.service");
/// The one global unit's label (ADR 0025: one `txtodod` per device, not one per workspace —
/// `migrate_old_units` finds and removes any leftover pre-M11 `{LABEL}.<8 hex>` units).
pub const LABEL: &str = "com.txtodo.txtodod";

/// What `txtodo daemon` can do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum Action {
    /// Write the one global service file for this device, migrating any pre-M11 per-workspace
    /// units it finds.
    Install,
    /// Load and start the service.
    Start,
    /// Stop and unload the service.
    Stop,
    /// Service state and whether the socket answers.
    Status,
}

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

/// Where a per-platform service integration keeps its unit files, and the extension its own
/// template uses — the two things `render`/`migrate_old_units` both need per platform.
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
    // Only the launchd template actually uses {{LOGDIR}} (systemd's own doc comment explains why:
    // stdout/stderr land in the user journal by systemd's own default instead).
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
/// effort per unit — a unit already stopped, or one `stop_by_label` fails against for any other
/// reason, still has its file removed and its label reported, never fatal to the install itself.
fn migrate_old_units(home: &Path) -> Vec<String> {
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

fn home_dir() -> Result<PathBuf, CliError> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| CliError::Message("txtodo daemon: $HOME is not set".into()))
}

fn txtodod_path() -> Result<PathBuf, CliError> {
    // Beside this binary, else whatever PATH resolves.
    let exe = std::env::current_exe().map_err(CliError::Io)?;
    let sibling = exe.with_file_name(format!("txtodod{}", std::env::consts::EXE_SUFFIX));
    Ok(if sibling.exists() {
        sibling
    } else {
        PathBuf::from("txtodod")
    })
}

fn run_ctl(program: &str, args: &[&str]) -> Result<(), CliError> {
    let status = Command::new(program)
        .args(args)
        .status()
        .map_err(CliError::Io)?;
    if status.success() {
        return Ok(());
    }
    Err(CliError::Message(format!(
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

/// Entry point for `txtodo daemon <action>`. One global unit (ADR 0025) — no workspace to resolve.
pub fn run(ctx: &Ctx, action: Action, force: bool) -> Result<(), CliError> {
    let home = home_dir()?;
    let Some(rendered) = render(&home, &txtodod_path()?) else {
        return Err(CliError::Message(
            "txtodo daemon: not supported on this platform yet (M10)".into(),
        ));
    };
    match action {
        Action::Install => install(&home, &rendered, force),
        Action::Start => start(&rendered),
        Action::Stop => stop(&rendered),
        Action::Status => status(ctx, &rendered),
    }
}

/// Migrates any pre-M11 per-workspace units, then writes the one global unit file.
fn install(home: &Path, r: &Rendered, force: bool) -> Result<(), CliError> {
    if r.path.exists() && !force {
        return Err(CliError::Message(format!(
            "txtodo daemon: {} exists; use --force to overwrite",
            r.path.display()
        )));
    }
    for label in migrate_old_units(home) {
        println!("migrated (removed) pre-M11 per-workspace unit {label}");
    }
    if let Some(dir) = r.path.parent() {
        std::fs::create_dir_all(dir).map_err(CliError::Io)?;
    }
    std::fs::write(&r.path, &r.body).map_err(CliError::Io)?;
    println!("installed {}", r.path.display());
    Ok(())
}

fn start(r: &Rendered) -> Result<(), CliError> {
    if !r.path.exists() {
        return Err(CliError::Message(format!(
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

fn stop(r: &Rendered) -> Result<(), CliError> {
    stop_by_label(&r.label)
}

fn stop_by_label(label: &str) -> Result<(), CliError> {
    if cfg!(target_os = "macos") {
        run_ctl("launchctl", &["bootout", &format!("gui/{}/{label}", uid())])
    } else {
        run_ctl(
            "systemctl",
            &["--user", "disable", "--now", &format!("{label}.service")],
        )
    }
}

fn status(ctx: &Ctx, r: &Rendered) -> Result<(), CliError> {
    let installed = if r.path.exists() {
        "installed"
    } else {
        "not installed"
    };
    let socket = ctx.paths.dir.join(SOCKET_REL);
    let env = crate::config::Env::from_process().map_err(CliError::Io)?;
    let answers = match client::select(&ctx.paths.dir, false, &env) {
        Ok(Mode::Daemon(mut d)) => d
            .health()
            .map(|h| format!("answers ({} document(s), v{})", h.documents, h.version))
            .unwrap_or_else(|e| e.to_string()),
        Ok(Mode::Direct) => "no socket".to_owned(),
        Err(e) => e.to_string(),
    };
    println!("service {} ({installed}, {})", r.label, r.path.display());
    println!("socket {} {answers}", socket.display());
    if answers.starts_with("answers") {
        Ok(())
    } else {
        Err(CliError::Reported)
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
}
