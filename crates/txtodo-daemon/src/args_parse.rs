//! Pure `txtodod` CLI argument parsers, split out of `main.rs` for its file-length budget.

use txtodo_model::IdentityMode;
use txtodo_sync::KeyStoreMode;

/// Lowercase (or uppercase) hex to exactly 32 bytes; `None` on anything else — `--relay-dial-peer`
/// is external input (a human or a test harness typed it), never assumed well-formed.
pub fn parse_relay_dial_peer(raw: &std::ffi::OsStr) -> Result<[u8; 32], String> {
    let s = raw
        .to_str()
        .ok_or_else(|| "--relay-dial-peer must be valid UTF-8 hex".to_owned())?;
    let bad = || format!("--relay-dial-peer must be 64 hex chars (32 bytes), got {s:?}");
    if s.len() != 64 {
        return Err(bad());
    }
    let mut out = [0u8; 32];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).map_err(|_| bad())?;
    }
    Ok(out)
}

/// `--identity-mode`'s value: `"tagged"` or `"sidecar"`.
pub fn parse_identity_mode(raw: &std::ffi::OsStr) -> Result<IdentityMode, String> {
    match raw.to_str() {
        Some("tagged") => Ok(IdentityMode::Tagged),
        Some("sidecar") => Ok(IdentityMode::Sidecar),
        _ => Err(format!(
            "--identity-mode must be tagged or sidecar, got {raw:?}"
        )),
    }
}

/// `--key-store`'s value: `"auto"`, `"os"` or `"file"`.
pub fn parse_key_store_mode(raw: &std::ffi::OsStr) -> Result<KeyStoreMode, String> {
    match raw.to_str() {
        Some("auto") => Ok(KeyStoreMode::Auto),
        Some("os") => Ok(KeyStoreMode::Os),
        Some("file") => Ok(KeyStoreMode::File),
        _ => Err(format!("--key-store must be auto, os or file, got {raw:?}")),
    }
}
