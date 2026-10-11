//! `txtodo upgrade [--check]` (task self-upgrade): moves this device to the newest release the way
//! it was installed. The newest tag comes from where GitHub's `releases/latest` redirects, fetched
//! with `curl` (no HTTP client in this crate, and only when the human asks: no background check).
//! How this copy was installed is read off the running binary's path: a Homebrew cellar, the
//! source build `just install` puts in `$CARGO_HOME/bin`, or else the curl script's
//! `~/.local/bin`. The TUI and desktop run this command; they hold no upgrade logic of their own.
//! Ref: https://docs.github.com/en/repositories/releasing-projects-on-github/linking-to-releases

use crate::CliError;
use crate::buildinfo::VERSION;
use std::path::{Path, PathBuf};
use std::process::Command;

const REPO: &str = "aaronmyatt/txtodo";
const BREW_FORMULA: &str = "aaronmyatt/tap/txtodo";
const INSTALL_SH: &str =
    "https://raw.githubusercontent.com/aaronmyatt/txtodo/main/scripts/install.sh";

/// How this copy of txtodo was installed, so it upgrades the same way.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Install {
    /// Homebrew's cellar: `brew upgrade`.
    Brew,
    /// `just install` from a checkout (`$CARGO_HOME/bin`): pull and rebuild there.
    Source,
    /// The curl script on macOS, re-run for the new tag into the folder these binaries sit in.
    Script { bin_dir: PathBuf },
    /// Anything else (a release binary copied by hand, Linux without brew): the release page.
    Manual,
}

/// Reads the install kind off `exe` (the canonical path of the running `txtodo`).
pub fn install_of(exe: &Path, macos: bool) -> Install {
    let text = exe.to_string_lossy();
    if text.contains("/Cellar/") || text.contains("/linuxbrew/") {
        return Install::Brew;
    }
    if text.contains("/.cargo/bin/") {
        return Install::Source;
    }
    match (macos, exe.parent()) {
        (true, Some(dir)) => Install::Script {
            bin_dir: dir.to_path_buf(),
        },
        _ => Install::Manual,
    }
}

/// The tag at the end of the URL `releases/latest` redirects to (`…/releases/tag/v0.0.23`).
pub fn tag_of(url: &str) -> Option<&str> {
    let tag = url.trim().rsplit_once("/releases/tag/")?.1;
    (!tag.is_empty() && !tag.contains('/')).then_some(tag)
}

/// `major.minor.patch` of a version or tag (`v0.0.23` or `0.0.23`); `None` when it is not one.
fn parts(v: &str) -> Option<(u64, u64, u64)> {
    let mut it = v.trim_start_matches('v').split('.').map(str::parse::<u64>);
    let out = (it.next()?.ok()?, it.next()?.ok()?, it.next()?.ok()?);
    it.next().is_none().then_some(out)
}

/// Whether `latest` is a newer release than `current`.
pub fn is_newer(latest: &str, current: &str) -> bool {
    matches!((parts(latest), parts(current)), (Some(l), Some(c)) if l > c)
}

/// What the human would type to do this upgrade by hand.
pub fn command_for(install: &Install, tag: &str) -> String {
    match install {
        Install::Brew => format!("brew upgrade {BREW_FORMULA}"),
        Install::Source => "git pull && just install   (in your txtodo checkout)".to_owned(),
        Install::Script { bin_dir } => format!(
            "curl -fsSL {INSTALL_SH} | TXTODO_VERSION={tag} TXTODO_BIN_DIR={} bash",
            bin_dir.display()
        ),
        Install::Manual => format!("download it from https://github.com/{REPO}/releases/tag/{tag}"),
    }
}

/// The newest release tag, from the redirect `releases/latest` answers with.
/// curl's `-w %{url_effective}`: https://curl.se/docs/manpage.html#-w
fn latest_tag() -> Result<String, CliError> {
    let out = Command::new("curl")
        .args(["-fsSL", "-o", "/dev/null", "-w", "%{url_effective}"])
        .arg(format!("https://github.com/{REPO}/releases/latest"))
        .output()
        .map_err(|e| CliError::Message(format!("upgrade needs curl: {e}")))?;
    if !out.status.success() {
        return Err(CliError::Message(
            "could not reach GitHub for the newest release".to_owned(),
        ));
    }
    let url = String::from_utf8_lossy(&out.stdout).into_owned();
    tag_of(&url)
        .map(str::to_owned)
        .ok_or_else(|| CliError::Message(format!("no release tag in GitHub's answer: {url}")))
}

/// `txtodo upgrade [--check]`.
pub fn run(check: bool) -> Result<(), CliError> {
    let tag = latest_tag()?;
    if !is_newer(&tag, VERSION) {
        println!("txtodo {VERSION} is the newest release ({tag}).");
        return Ok(());
    }
    let exe = std::env::current_exe()
        .and_then(std::fs::canonicalize)
        .map_err(CliError::Io)?;
    let install = install_of(&exe, cfg!(target_os = "macos"));
    println!("txtodo {VERSION} → {tag}");
    println!("  {}", command_for(&install, &tag));
    if check {
        return Ok(());
    }
    upgrade(&install, &tag, &exe)
}

/// Runs the upgrade for `install`, then restarts the daemon with the new `txtodod`.
fn upgrade(install: &Install, tag: &str, exe: &Path) -> Result<(), CliError> {
    let status = match install {
        Install::Brew => Command::new("brew")
            .args(["upgrade", BREW_FORMULA])
            .status(),
        Install::Script { bin_dir } => Command::new("bash")
            .args(["-c", &format!("curl -fsSL {INSTALL_SH} | bash")])
            .env("TXTODO_VERSION", tag)
            .env("TXTODO_BIN_DIR", bin_dir)
            .status(),
        Install::Source | Install::Manual => {
            return Err(CliError::Message(format!(
                "this copy cannot upgrade itself: {}",
                command_for(install, tag)
            )));
        }
    }
    .map_err(CliError::Io)?;
    if !status.success() {
        return Err(CliError::Message(format!("the upgrade failed ({status})")));
    }
    // The old txtodod keeps serving until its service restarts on the new binary. The path is the
    // same file, now the new build (brew's `bin` link may move: resolve it again).
    let txtodo = which_txtodo().unwrap_or_else(|| exe.to_path_buf());
    let restarted = Command::new(&txtodo)
        .args(["daemon", "install", "--force"])
        .status()
        .map_err(CliError::Io)?;
    if !restarted.success() {
        eprintln!(
            "txtodo: upgraded, but restarting the daemon failed: run `txtodo daemon install --force`"
        );
    }
    println!("txtodo: upgraded to {tag}.");
    Ok(())
}

/// The `txtodo` on `$PATH` now, so a brew upgrade's new cellar path is the one that restarts.
fn which_txtodo() -> Option<PathBuf> {
    let out = Command::new("sh")
        .args(["-c", "command -v txtodo"])
        .output()
        .ok()?;
    let path = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    (out.status.success() && !path.is_empty()).then(|| PathBuf::from(path))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tag_is_the_end_of_the_redirect() {
        assert_eq!(
            tag_of("https://github.com/aaronmyatt/txtodo/releases/tag/v0.0.23\n"),
            Some("v0.0.23")
        );
        assert_eq!(
            tag_of("https://github.com/aaronmyatt/txtodo/releases"),
            None
        );
        assert_eq!(tag_of("https://github.com/x/releases/tag/"), None);
    }

    #[test]
    fn newer_compares_numbers_not_text() {
        assert!(is_newer("v0.0.23", "0.0.22"));
        assert!(is_newer("v0.1.0", "0.0.99"));
        assert!(!is_newer("v0.0.22", "0.0.22"));
        assert!(!is_newer("v0.0.9", "0.0.22"), "9 < 22 as numbers");
        assert!(!is_newer("nightly", "0.0.22"));
    }

    #[test]
    fn the_install_kind_comes_from_the_binary_path() {
        let brew = Path::new("/opt/homebrew/Cellar/txtodo/0.0.20/bin/txtodo");
        assert_eq!(install_of(brew, true), Install::Brew);
        let source = Path::new("/Users/me/.cargo/bin/txtodo");
        assert_eq!(install_of(source, true), Install::Source);
        let script = Path::new("/Users/me/.local/bin/txtodo");
        assert_eq!(
            install_of(script, true),
            Install::Script {
                bin_dir: PathBuf::from("/Users/me/.local/bin")
            }
        );
        assert_eq!(
            install_of(script, false),
            Install::Manual,
            "the script is macOS only"
        );
    }

    #[test]
    fn the_printed_command_names_the_tag_and_folder() {
        let script = Install::Script {
            bin_dir: PathBuf::from("/Users/me/.local/bin"),
        };
        let cmd = command_for(&script, "v0.0.23");
        assert!(cmd.contains("TXTODO_VERSION=v0.0.23"), "{cmd}");
        assert!(cmd.contains("TXTODO_BIN_DIR=/Users/me/.local/bin"), "{cmd}");
        assert_eq!(
            command_for(&Install::Brew, "v0.0.23"),
            "brew upgrade aaronmyatt/tap/txtodo"
        );
    }
}
