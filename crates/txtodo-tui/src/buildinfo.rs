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

#[cfg(test)]
mod tests {
    use super::*;

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
