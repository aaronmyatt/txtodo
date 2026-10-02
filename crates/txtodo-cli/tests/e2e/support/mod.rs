//! Shared by every CLI integration test that spawns a real `txtodod`: one copy of the lookup
//! instead of six, so a fix to it lands everywhere.

use std::path::{Path, PathBuf};
use std::process::Command;

/// The `txtodod` built beside this test binary (`target/<profile>/`), building it first when it
/// is missing or empty.
///
/// Empty, not just missing: apps/desktop's build.rs leaves a 0-byte executable placeholder at
/// `target/<profile>/txtodod` (tauri's externalBin copy) that CI's `--exclude txtodo-daemon` never
/// overwrites; exec of it is ENOEXEC on linux and a silent no-op on macOS.
///
/// `--target-dir` is passed explicitly: under `cargo llvm-cov` this test binary lives in
/// `target/llvm-cov-target/`, and a nested `cargo build` without it writes to plain `target/`,
/// leaving the empty placeholder that was just checked.
/// Ref: https://doc.rust-lang.org/cargo/commands/cargo-build.html#output-options
pub fn txtodod_binary() -> PathBuf {
    let mut dir = std::env::current_exe().unwrap_or_else(|e| panic!("current_exe: {e}"));
    dir.pop();
    if dir.ends_with("deps") {
        dir.pop();
    }
    let bin = dir.join(format!("txtodod{}", std::env::consts::EXE_SUFFIX));
    if !is_built(&bin) {
        let target = dir
            .parent()
            .unwrap_or_else(|| panic!("{} has no parent", dir.display()));
        let mut build = Command::new(env!("CARGO"));
        build
            .args([
                "build",
                "-p",
                "txtodo-daemon",
                "--bin",
                "txtodod",
                "--quiet",
            ])
            .arg("--target-dir")
            .arg(target);
        if dir.ends_with("release") {
            build.arg("--release");
        }
        let status = build
            .status()
            .unwrap_or_else(|e| panic!("cargo build txtodod: {e}"));
        assert!(status.success(), "building txtodod failed");
    }
    assert!(
        is_built(&bin),
        "{} is missing or empty after building it",
        bin.display()
    );
    bin
}

fn is_built(bin: &Path) -> bool {
    bin.metadata().is_ok_and(|m| m.len() > 0)
}

pub mod global_daemon;
