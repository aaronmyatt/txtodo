//! This build's version and release date (task version-info), resolved at build time by `build.rs`
//! (`build-support/buildinfo.rs`): `$TXTODO_RELEASE_DATE`, else the last commit's date, else
//! `unknown`. `Health` sends both, so a client can tell it is talking to another build; the boot
//! log records both, so a log says which build wrote it.

/// The workspace version. https://doc.rust-lang.org/cargo/reference/environment-variables.html
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
/// `YYYY-MM-DD`, or `unknown` for a build with neither source.
pub const RELEASE_DATE: &str = env!("TXTODO_RELEASE_DATE");
/// What follows the program's name in `--version`: `0.0.2 (2026-09-20)`.
/// `concat!` takes literals and `env!` only: https://doc.rust-lang.org/std/macro.concat.html
pub const VERSION_LINE: &str = concat!(
    env!("CARGO_PKG_VERSION"),
    " (",
    env!("TXTODO_RELEASE_DATE"),
    ")"
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_version_line_is_the_version_and_the_date() {
        assert_eq!(VERSION_LINE, format!("{VERSION} ({RELEASE_DATE})"));
        assert!(
            RELEASE_DATE == "unknown" || RELEASE_DATE.len() == 10,
            "a date or unknown: {RELEASE_DATE}"
        );
    }
}
