//! Tauri's own build step: embeds `tauri.conf.json` and writes the capability schemas under
//! `gen/schemas/` for editor autocompletion. Ref: https://v2.tauri.app/reference/config/
use std::path::PathBuf;

fn main() {
    ensure_sidecar_placeholder();
    tauri_build::build()
}

/// tauri.conf.json's `bundle.externalBin: ["binaries/txtodod"]` (task
/// desktop-daemon-sidecar-bundle) makes `tauri_build::build()` hard-require
/// `binaries/txtodod-<target-triple>[.exe]` to exist on disk for *any* cargo build/check/test/
/// clippy that compiles this crate — not only a real `tauri build`/`tauri dev` bundle. Real
/// packaging (release.yml's build-desktop job, or `just stage-desktop-sidecar` for a local
/// `npm run tauri build`) always stages the real cross-compiled `txtodod` binary here first,
/// overwriting any placeholder. This only fills the gap for everyone else — `cargo build
/// --workspace`, CI's `check` job, a fresh clone following the root README's "From source"
/// instructions — none of which read the sidecar's contents, only whether the path exists.
fn ensure_sidecar_placeholder() {
    let Ok(target) = std::env::var("TARGET") else {
        return;
    };
    let ext = if target.contains("windows") {
        ".exe"
    } else {
        ""
    };
    let path = PathBuf::from("binaries").join(format!("txtodod-{target}{ext}"));
    if path.exists() {
        return;
    }
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    // Needed on unix (skips permission-setting below on write failure); on non-unix targets
    // there's nothing left in the function afterward, so clippy sees this `return` as needless —
    // it isn't, once `#[cfg(unix)]` is accounted for.
    #[cfg_attr(not(unix), allow(clippy::needless_return))]
    if std::fs::write(&path, []).is_err() {
        return;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = std::fs::metadata(&path) {
            let mut perms = meta.permissions();
            perms.set_mode(0o755);
            let _ = std::fs::set_permissions(&path, perms);
        }
    }
}
