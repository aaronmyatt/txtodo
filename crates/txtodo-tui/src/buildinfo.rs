//! This build's version and release date (task version-info), resolved at build time by `build.rs`
//! (`build-support/buildinfo.rs`): `$TXTODO_RELEASE_DATE`, else the last commit's date, else
//! `unknown`. `--version` prints `txtodo-tui 0.0.2 (2026-09-20)`; the status bar shows the short
//! form, dim, and drops it first when the terminal is narrow.
//! `concat!` takes literals and `env!` only: https://doc.rust-lang.org/std/macro.concat.html

/// What follows the program's name in `--version`.
pub const VERSION_LINE: &str = concat!(
    env!("CARGO_PKG_VERSION"),
    " (",
    env!("TXTODO_RELEASE_DATE"),
    ")"
);

/// The status bar's form: `v0.0.2 · 2026-09-20`.
pub const UI_LABEL: &str = concat!(
    "v",
    env!("CARGO_PKG_VERSION"),
    " \u{b7} ",
    env!("TXTODO_RELEASE_DATE")
);

/// The daemon's version and release date when they are not this build's (the version banner, task
/// `tui-revamp/tui-shell`, as desktop's `daemonMismatch`). A daemon that sends no release date is
/// older than the field, so it counts as different; one that sends no version is too old to say.
pub fn other_build(version: &str, release_date: &str) -> Option<(String, String)> {
    let same = version == env!("CARGO_PKG_VERSION") && release_date == env!("TXTODO_RELEASE_DATE");
    if version.is_empty() || same {
        return None;
    }
    let date = if release_date.is_empty() {
        "an older build"
    } else {
        release_date
    };
    Some((version.to_owned(), date.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_another_build_is_reported() {
        let (version, date) = (env!("CARGO_PKG_VERSION"), env!("TXTODO_RELEASE_DATE"));
        assert_eq!(other_build(version, date), None);
        assert_eq!(other_build("", ""), None, "too old to say");
        assert_eq!(
            other_build(version, ""),
            Some((version.to_owned(), "an older build".to_owned()))
        );
        assert_eq!(
            other_build("0.0.1", "2026-01-01"),
            Some(("0.0.1".to_owned(), "2026-01-01".to_owned()))
        );
    }

    #[test]
    fn both_forms_carry_the_version_and_the_same_date() {
        let version = env!("CARGO_PKG_VERSION");
        let date = env!("TXTODO_RELEASE_DATE");
        assert_eq!(VERSION_LINE, format!("{version} ({date})"));
        assert_eq!(UI_LABEL, format!("v{version} \u{b7} {date}"));
        assert!(
            date == "unknown" || date.len() == 10,
            "a date or unknown: {date}"
        );
    }
}
