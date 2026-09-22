//! Restarting a running global daemon that is older than the client (task
//! `daemon-auto-upgrade`): `txtodod` writes `txtodod.version` beside its pid file, a client hands
//! [`crate::LaunchConfig::upgrade_to`] its own version, and [`crate::ensure_daemon`] restarts the
//! daemon with the resolved newer binary when both the client and that binary are newer than
//! what runs. Never a downgrade, never a restart loop: the binary about to be spawned is asked
//! for its own `--version` first, so a stale `$PATH` copy behind a newer client does nothing.
//!
//! The pure decision ([`decide`], [`parse_version`]) lives here for every platform; the restart
//! itself is `spawn.rs`'s unix-only business.

use std::path::Path;
use std::process::Command;

/// `major.minor.patch`, numeric, from the first digit-led token of `text` — `"0.0.7"`,
/// `"txtodod 0.0.7 (2026-09-22)"` and `"v0.0.7"` all parse to `(0, 0, 7)`. Anything else is
/// `None`, and `None` never triggers a restart.
pub(crate) fn parse_version(text: &str) -> Option<(u64, u64, u64)> {
    let token = text
        .split_whitespace()
        .map(|t| t.trim_start_matches('v'))
        .find(|t| t.starts_with(|c: char| c.is_ascii_digit()))?;
    let mut parts = token.split('.').map(|p| {
        p.trim_end_matches(|c: char| !c.is_ascii_digit())
            .parse::<u64>()
            .ok()
    });
    let major = parts.next()??;
    let minor = parts.next().flatten().unwrap_or(0);
    let patch = parts.next().flatten().unwrap_or(0);
    Some((major, minor, patch))
}

/// Whether to restart, given the running daemon's version, the client's own, and the version the
/// binary about to be spawned reports. Restart only when the client is newer than what runs
/// (the client's claim) *and* the binary is newer too (the proof); a missing or unparsable
/// version anywhere is a no.
pub(crate) fn decide(running: &str, client: &str, binary: Option<&str>) -> bool {
    let (Some(running), Some(client), Some(binary)) = (
        parse_version(running),
        parse_version(client),
        binary.and_then(parse_version),
    ) else {
        return false;
    };
    client > running && binary > running
}

/// The version of the daemon holding `socket`, read from `txtodod.version` in the socket's
/// directory. A state dir with a pid file but no version file is a daemon from before this file
/// existed, so it reads as `0.0.0` (older than anything); no pid file means this is not a
/// `txtodod` state dir at all (`None`).
pub(crate) fn running_version(socket: &Path) -> Option<String> {
    let dir = socket.parent()?;
    match std::fs::read_to_string(dir.join("txtodod.version")) {
        Ok(text) => Some(text.trim().to_owned()),
        Err(_) if dir.join("txtodod.pid").exists() => Some("0.0.0".to_owned()),
        Err(_) => None,
    }
}

/// `<bin> --version`'s first line, or `None` when the binary cannot be run — which then reads as
/// "do not restart" in [`decide`].
pub(crate) fn binary_version(bin: &Path) -> Option<String> {
    let out = Command::new(bin).arg("--version").output().ok()?;
    if !out.status.success() {
        return None;
    }
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .next()
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_bare_prefixed_and_labelled_versions() {
        assert_eq!(parse_version("0.0.7"), Some((0, 0, 7)));
        assert_eq!(parse_version("v1.2.3"), Some((1, 2, 3)));
        assert_eq!(parse_version("txtodod 0.0.7 (2026-09-22)"), Some((0, 0, 7)));
        assert_eq!(parse_version("0.1.0-rc1"), Some((0, 1, 0)));
        assert_eq!(parse_version("unknown"), None);
        assert_eq!(parse_version(""), None);
    }

    #[test]
    fn restarts_only_when_client_and_binary_are_both_newer() {
        assert!(decide("0.0.6", "0.0.7", Some("txtodod 0.0.7 (x)")));
        assert!(!decide("0.0.7", "0.0.7", Some("0.0.7")), "same build");
        assert!(
            !decide("0.0.8", "0.0.7", Some("0.0.7")),
            "never a downgrade"
        );
        assert!(
            !decide("0.0.6", "0.0.7", Some("0.0.6")),
            "a stale $PATH binary behind a newer client would loop"
        );
        assert!(!decide("0.0.6", "0.0.7", None), "no binary version");
        assert!(!decide("garbage", "0.0.7", Some("0.0.7")));
    }

    #[test]
    fn running_version_reads_the_file_and_falls_back_on_a_pid_file() {
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
        let socket = dir.path().join("txtodod.sock");
        assert_eq!(running_version(&socket), None, "not a txtodod state dir");
        std::fs::write(dir.path().join("txtodod.pid"), "123").unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(running_version(&socket).as_deref(), Some("0.0.0"));
        std::fs::write(dir.path().join("txtodod.version"), "0.0.7\n")
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(running_version(&socket).as_deref(), Some("0.0.7"));
    }
}
