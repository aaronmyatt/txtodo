//! This build's version and release date, as every txtodo program shows them (task version-info):
//! `0.0.2 (2026-09-20)` after `--version`, `v0.0.2 · 2026-09-20` in a UI. The date is resolved at
//! build time by `build.rs` (`build-support/buildinfo.rs`): the release workflow's
//! `$TXTODO_RELEASE_DATE`, else the last commit's date, else `unknown`.

/// The workspace version. https://doc.rust-lang.org/cargo/reference/environment-variables.html
/// Test-only until `doctor` compares it with the daemon's (a later line of task version-info).
#[cfg(test)]
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
/// `YYYY-MM-DD`, or `unknown` for a build with neither source. Test-only for now, like `VERSION`.
#[cfg(test)]
pub const RELEASE_DATE: &str = env!("TXTODO_RELEASE_DATE");
/// What follows the program's name in `--version` and at the top of `doctor`.
/// `concat!` takes literals and `env!` only: https://doc.rust-lang.org/std/macro.concat.html
pub const VERSION_LINE: &str = concat!(
    env!("CARGO_PKG_VERSION"),
    " (",
    env!("TXTODO_RELEASE_DATE"),
    ")"
);

// The build script's own logic, included the way the build scripts include it, so its fallback
// order is tested without making it a crate. A `#[path]` outside an inline module is relative to
// this file's directory: https://doc.rust-lang.org/reference/items/modules.html#the-path-attribute
#[cfg(test)]
#[path = "../../../build-support/buildinfo.rs"]
mod build_script;

#[cfg(test)]
mod tests {
    use super::*;

    use super::build_script::{UNKNOWN, is_date, release_date};

    #[test]
    fn the_env_var_wins_then_the_commit_date_then_unknown() {
        let git = || Some("2026-09-19\n".to_owned());
        assert_eq!(release_date(Some("2026-09-20"), git), "2026-09-20");
        assert_eq!(release_date(None, git), "2026-09-19");
        assert_eq!(
            release_date(Some("  "), git),
            "2026-09-19",
            "an empty var is no date"
        );
        assert_eq!(
            release_date(Some("v0.0.3"), git),
            "2026-09-19",
            "a tag is no date"
        );
        assert_eq!(release_date(None, || None), UNKNOWN);
        assert_eq!(
            release_date(None, || Some("fatal: not a git repo".into())),
            UNKNOWN
        );
    }

    #[test]
    fn the_commit_date_is_not_asked_when_the_env_var_gave_one() {
        let date = release_date(Some("2026-09-20"), || panic!("git must not run"));
        assert_eq!(date, "2026-09-20");
    }

    #[test]
    fn only_a_ten_char_iso_date_is_a_date() {
        assert!(is_date("2026-09-20"));
        for bad in [
            "",
            "2026-9-20",
            "2026/09/20",
            "20260920",
            "2026-09-20T10",
            "unknown",
        ] {
            assert!(!is_date(bad), "{bad:?}");
        }
    }

    #[test]
    fn this_build_shows_its_version_and_a_date_or_unknown() {
        assert_eq!(VERSION_LINE, format!("{VERSION} ({RELEASE_DATE})"));
        assert!(
            is_date(RELEASE_DATE) || RELEASE_DATE == UNKNOWN,
            "{RELEASE_DATE}"
        );
    }
}
