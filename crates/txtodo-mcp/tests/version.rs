//! `txtodo-mcp --version` (task version-info): exits 0 and prints the name, the crate version and
//! a date, the shape release.yml's smoke test checks on every shipped binary.
//! `CARGO_BIN_EXE_<name>`: https://doc.rust-lang.org/cargo/reference/environment-variables.html#environment-variables-cargo-sets-for-crates

use std::process::Command;

#[test]
fn version_prints_name_version_and_date() {
    let out = Command::new(env!("CARGO_BIN_EXE_txtodo-mcp"))
        .arg("--version")
        .output()
        .expect("run txtodo-mcp --version");
    assert!(out.status.success(), "exit status {:?}", out.status);
    let line = String::from_utf8(out.stdout).expect("utf-8 stdout");
    let prefix = format!("txtodo-mcp {} (", env!("CARGO_PKG_VERSION"));
    assert!(
        line.starts_with(&prefix),
        "unexpected --version line: {line:?}"
    );
    assert!(line.trim_end().ends_with(')'), "no date in: {line:?}");
}
