//! `txtodo daemon install|start|stop|status` (plan M3): render the platform template from
//! `deploy/` with the daemon binary and workspace, hand it to launchd or systemd --user, and probe
//! the socket for status. Windows is M10. Templates are embedded so the binary is self-contained.

use crate::client::{self, Mode, SOCKET_REL};
use crate::{CliError, Ctx};
use std::path::{Path, PathBuf};
use std::process::Command;

/// The launchd agent template (`deploy/launchd/`).
pub const LAUNCHD_TEMPLATE: &str =
    include_str!("../../../../deploy/launchd/com.txtodo.txtodod.plist");
/// The systemd user unit template (`deploy/systemd/`).
pub const SYSTEMD_TEMPLATE: &str = include_str!("../../../../deploy/systemd/txtodod.service");
/// Service label prefix; a short hash of the workspace path follows so two workspaces can coexist.
pub const LABEL_PREFIX: &str = "com.txtodo.txtodod";

/// What `txtodo daemon` can do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum Action {
    /// Write the service file for this workspace.
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

/// `com.txtodo.txtodod.<8 hex of a stable hash of the workspace path>`.
pub fn label_for(workspace: &Path) -> String {
    // FNV-1a over the path bytes: stable, dependency-free, plenty for a per-workspace suffix.
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in workspace.to_string_lossy().bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    let label = format!("{LABEL_PREFIX}.{:08x}", h as u32);
    debug_assert!(label.starts_with(LABEL_PREFIX) && label.len() == LABEL_PREFIX.len() + 9);
    label
}

/// Fills the placeholders of a template.
pub fn render_template(template: &str, label: &str, txtodod: &Path, workspace: &Path) -> String {
    let out = template
        .replace("{{LABEL}}", label)
        .replace("{{TXTODOD}}", &txtodod.to_string_lossy())
        .replace("{{WORKSPACE}}", &workspace.to_string_lossy());
    debug_assert!(!out.contains("{{"), "every placeholder is filled");
    out
}

/// The service file for this platform, or `None` where txtodo has no service integration yet.
pub fn render(home: &Path, txtodod: &Path, workspace: &Path) -> Option<Rendered> {
    let label = label_for(workspace);
    if cfg!(target_os = "macos") {
        let path = home
            .join("Library")
            .join("LaunchAgents")
            .join(format!("{label}.plist"));
        return Some(Rendered {
            body: render_template(LAUNCHD_TEMPLATE, &label, txtodod, workspace),
            label,
            path,
        });
    }
    if cfg!(target_os = "linux") {
        let path = home
            .join(".config")
            .join("systemd")
            .join("user")
            .join(format!("{label}.service"));
        return Some(Rendered {
            body: render_template(SYSTEMD_TEMPLATE, &label, txtodod, workspace),
            label,
            path,
        });
    }
    None
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

/// Entry point for `txtodo daemon <action>`.
pub fn run(ctx: &Ctx, action: Action, force: bool) -> Result<(), CliError> {
    let workspace = ctx
        .paths
        .dir
        .canonicalize()
        .unwrap_or_else(|_| ctx.paths.dir.clone());
    let Some(rendered) = render(&home_dir()?, &txtodod_path()?, &workspace) else {
        return Err(CliError::Message(
            "txtodo daemon: not supported on this platform yet (M10)".into(),
        ));
    };
    match action {
        Action::Install => install(&rendered, force),
        Action::Start => start(&rendered),
        Action::Stop => stop(&rendered),
        Action::Status => status(ctx, &rendered),
    }
}

fn install(r: &Rendered, force: bool) -> Result<(), CliError> {
    if r.path.exists() && !force {
        return Err(CliError::Message(format!(
            "txtodo daemon: {} exists; use --force to overwrite",
            r.path.display()
        )));
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
    if cfg!(target_os = "macos") {
        run_ctl(
            "launchctl",
            &["bootout", &format!("gui/{}/{}", uid(), r.label)],
        )
    } else {
        run_ctl(
            "systemctl",
            &[
                "--user",
                "disable",
                "--now",
                &format!("{}.service", r.label),
            ],
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
    let answers = match client::select(&ctx.paths.dir, false) {
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
    fn labels_are_stable_per_workspace_and_templates_fill_every_placeholder() {
        let a = label_for(Path::new("/w/one"));
        assert_eq!(a, label_for(Path::new("/w/one")));
        assert_ne!(a, label_for(Path::new("/w/two")));
        let body = render_template(
            LAUNCHD_TEMPLATE,
            &a,
            Path::new("/usr/local/bin/txtodod"),
            Path::new("/w/one"),
        );
        assert!(
            body.contains("<string>/usr/local/bin/txtodod</string>")
                && body.contains("<string>/w/one</string>")
        );
        assert!(!body.contains("{{"));
        let unit = render_template(
            SYSTEMD_TEMPLATE,
            &a,
            Path::new("/bin/txtodod"),
            Path::new("/w/one"),
        );
        assert!(unit.contains("ExecStart=/bin/txtodod --dir /w/one"));
        let r = render(
            Path::new("/home/u"),
            Path::new("/bin/txtodod"),
            Path::new("/w/one"),
        );
        if cfg!(any(target_os = "macos", target_os = "linux")) {
            let r = r.unwrap_or_else(|| panic!("supported platform"));
            assert!(r.path.starts_with("/home/u"));
            assert!(r.path.to_string_lossy().contains(&a));
        }
    }
}
