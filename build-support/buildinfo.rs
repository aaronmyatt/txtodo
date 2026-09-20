//! The release date every txtodo binary shows next to its version (task version-info). Not a
//! crate: each `build.rs` includes this file with `#[path]`, so there is no new workspace member
//! and no new dependency edge. It resolves the date once, at build time, and hands it to the
//! crate as the compile-time env var `TXTODO_RELEASE_DATE` (read with `env!`).
//!
//! Order: `$TXTODO_RELEASE_DATE` (the release workflow sets it from the tag), else the date of the
//! last commit, else `unknown`. Never the build time: two builds of one tag must be byte-identical,
//! and the release workflow compares them.
//! Build scripts: https://doc.rust-lang.org/cargo/reference/build-scripts.html

/// The compile-time env var the crates read, and the env var a release build sets.
pub const RELEASE_DATE_VAR: &str = "TXTODO_RELEASE_DATE";
/// What a build shows when neither source gave a date (a source tarball with no `.git`).
pub const UNKNOWN: &str = "unknown";

/// `YYYY-MM-DD`, digits and dashes in the right places. Anything else is not shown as a date.
pub fn is_date(text: &str) -> bool {
    let b = text.as_bytes();
    b.len() == 10
        && b.iter().enumerate().all(|(i, c)| {
            if i == 4 || i == 7 {
                *c == b'-'
            } else {
                c.is_ascii_digit()
            }
        })
}

/// The date to show: `from_env` when it is a date, else `from_git()` when that is one, else
/// [`UNKNOWN`]. `from_git` is only called when the env var gave nothing usable.
pub fn release_date(from_env: Option<&str>, from_git: impl FnOnce() -> Option<String>) -> String {
    if let Some(date) = from_env.map(str::trim).filter(|d| is_date(d)) {
        return date.to_owned();
    }
    from_git()
        .map(|d| d.trim().to_owned())
        .filter(|d| is_date(d))
        .unwrap_or_else(|| UNKNOWN.to_owned())
}

/// The last commit's date, `%cs` = committer date as `YYYY-MM-DD`. `None` with no git or no repo.
/// https://git-scm.com/docs/git-show#Documentation/git-show.txt-emcsem
#[allow(dead_code)] // a test that includes this file calls `release_date` with its own sources
pub fn commit_date() -> Option<String> {
    let out = std::process::Command::new("git")
        .args(["show", "-s", "--format=%cs", "HEAD"])
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// The file git appends to on every commit and checkout, asked of git itself so a worktree (where
/// `.git` is a file, not a directory) resolves too. `None` with no git or no repo.
/// https://git-scm.com/docs/git-rev-parse#Documentation/git-rev-parse.txt---git-pathltpathgt
#[allow(dead_code)]
fn head_log_path() -> Option<String> {
    let out = std::process::Command::new("git")
        .args(["rev-parse", "--git-path", "logs/HEAD"])
        .output()
        .ok()?;
    let path = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    (out.status.success() && !path.is_empty()).then_some(path)
}

/// What a `build.rs` calls: resolves the date and hands it to the crate being built. The script
/// runs again when the env var changes or when HEAD moves (a new commit, a checkout).
/// https://doc.rust-lang.org/cargo/reference/build-scripts.html#rustc-env
/// https://doc.rust-lang.org/cargo/reference/build-scripts.html#rerun-if-changed
#[allow(dead_code)]
pub fn emit() {
    let from_env = std::env::var(RELEASE_DATE_VAR).ok();
    let date = release_date(from_env.as_deref(), commit_date);
    println!("cargo:rustc-env={RELEASE_DATE_VAR}={date}");
    println!("cargo:rerun-if-env-changed={RELEASE_DATE_VAR}");
    if let Some(head_log) = head_log_path() {
        println!("cargo:rerun-if-changed={head_log}");
    }
}
