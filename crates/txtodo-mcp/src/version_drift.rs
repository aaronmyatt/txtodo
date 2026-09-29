//! Whether this server is older than the daemon it talks to (task mcp-version-drift). An MCP client
//! starts `txtodo-mcp` over stdio and keeps it for the whole session: nothing restarts it when a
//! new build is installed, while `txtodod` is restarted by the next newer client (task
//! daemon-auto-upgrade). `main.rs` asks the daemon's version at start and every few minutes and
//! shows [`warning`]'s text, once per daemon version.

/// `(major, minor, patch)` of `a.b.c`, ignoring a `-pre`/`+build` suffix; `None` for anything else.
/// Semantic versions: <https://semver.org/#spec-item-2>
fn parse(version: &str) -> Option<(u64, u64, u64)> {
    let core = version.split(['-', '+']).next()?;
    let mut parts = core.split('.').map(|p| p.parse::<u64>().ok());
    let parsed = (parts.next()??, parts.next()??, parts.next()??);
    parts.next().is_none().then_some(parsed)
}

/// The warning for a daemon at `daemon` when this server is `own`: `None` unless the daemon is
/// newer, or when `warned` already names that daemon version. A version that does not parse never
/// warns: a wrong nag is worse than none.
pub fn warning(own: &str, daemon: &str, warned: Option<&str>) -> Option<String> {
    if warned == Some(daemon) || parse(daemon)? <= parse(own)? {
        return None;
    }
    Some(format!(
        "txtodo-mcp {own} is older than the txtodod {daemon} it talks to; reconnect this MCP \
         server in your client (or start a new session) to run the new build"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_newer_daemon_warns_once_per_version() {
        let first = warning("0.0.16", "0.0.17", None);
        assert!(first.is_some_and(|w| w.contains("0.0.16") && w.contains("0.0.17")));
        assert_eq!(
            warning("0.0.16", "0.0.17", Some("0.0.17")),
            None,
            "already warned"
        );
        assert!(
            warning("0.0.16", "0.1.0", Some("0.0.17")).is_some(),
            "a newer one again"
        );
        assert!(warning("0.9.9", "1.0.0", None).is_some());
    }

    #[test]
    fn a_same_older_or_unreadable_daemon_never_warns() {
        assert_eq!(warning("0.0.17", "0.0.17", None), None);
        assert_eq!(warning("0.0.17", "0.0.16", None), None);
        assert_eq!(
            warning("0.0.17", "", None),
            None,
            "an old daemon that sends no version"
        );
        assert_eq!(warning("0.0.17", "dev", None), None);
        assert_eq!(
            warning("0.0.17-rc1", "0.0.17", None),
            None,
            "a suffix is ignored"
        );
    }
}
