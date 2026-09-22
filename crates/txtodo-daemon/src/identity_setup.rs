//! `build_identity`/`prompt_file_passphrase`, split out of `main.rs` for its file-length budget.

use std::io::Write;
use std::path::Path;

use txtodo_daemon::clock::Clock;
use txtodo_daemon::device_identity::DeviceIdentity;
use txtodo_sync::{KeyStoreMode, Secret};

use crate::Args;

/// Reads a passphrase for `--key-store file` as one line from stdin — never a CLI argument or
/// environment variable (CLAUDE.md §3.1), so `ps`/shell history never carries it. The `String`'s
/// buffer moves directly into `Secret` (zeroized on drop) via `into_bytes`, no extra copy.
/// **Known gap**: does not suppress terminal echo (needs `rpassword`, not added without sign-off —
/// see tasks/sync-keystore/notes.md's "As built"); the CLAUDE.md §3.1 properties still hold.
pub(crate) fn prompt_file_passphrase() -> Result<Secret, Box<dyn std::error::Error>> {
    eprint!("txtodod: key_store = \"file\" passphrase: ");
    std::io::stderr().flush().ok();
    let mut line = String::new();
    std::io::stdin().read_line(&mut line)?;
    let kept = line.trim_end_matches(['\n', '\r']).len();
    line.truncate(kept);
    if line.is_empty() {
        return Err("no passphrase read from stdin; key_store = \"file\" needs one".into());
    }
    Ok(Secret::new(line.into_bytes()))
}

/// Test-only escape hatch, same `TXTODO_..._ENV_VAR = "1"` gate idiom as
/// `debug_hooks::TEST_HOOKS_ENV_VAR`: forces the pure in-memory keystore this crate's (and
/// `txtodo-cli`/`txtodo-tui`/`txtodo-mcp`/`apps/desktop`'s) real-daemon integration tests relied on
/// before task `relay-id-keystore` flipped the no-flag default to `auto` — without it, every one of
/// those tests would start probing the real OS keychain on every spawn, the exact
/// OS-keychain-reachability dependence this crate's own tests are supposed to never have. Set
/// workspace-wide in `.cargo/config.toml`'s `[env]` (same mechanism as `TXTODO_NO_SERVICE`) so a
/// spawned `txtodod` inherits it from whichever test process spawned it, with no per-harness
/// plumbing; an explicit `--key-store` flag still wins over it, same as the real default.
pub(crate) const TEST_MEMORY_KEYSTORE_ENV_VAR: &str = "TXTODO_TEST_KEYSTORE_MEMORY";

/// The identity plus what it allows (task keystore-memory-fallback).
pub(crate) struct BuiltIdentity {
    pub(crate) identity: DeviceIdentity,
    /// False when the keys live in the `auto` fallback's in-memory keystore: the device static,
    /// signing and group keys would be reminted at the next start under the same persisted
    /// device id, breaking every paired peer's handshake, so relay and LAN sync stay off for
    /// this run rather than pair a device that cannot keep its keys.
    pub(crate) sync_allowed: bool,
}

/// Whether a daemon on `backend` may hold a sync group. The test seam's memory keystore
/// (`via_test_seam`) is allowed to: a hermetic two-daemon test pairs in one run and never
/// restarts into the same identity.
pub(crate) fn sync_allowed_for(backend: &str, via_test_seam: bool) -> bool {
    via_test_seam || backend != "memory"
}

/// The test seam, honoured in debug builds only (task keystore-memory-fallback): a release
/// `txtodod` (`just install-daemon`, the desktop sidecar) never reads the variable, so a
/// developer's `.cargo/config.toml` can no longer hand a shipped daemon volatile keys. A debug
/// `cargo run` still picks it up — it is the same binary the tests spawn — and is told so.
fn test_seam_requested(args: &Args) -> bool {
    if !cfg!(debug_assertions) || args.key_store_mode.is_some() {
        return false;
    }
    let on = std::env::var(TEST_MEMORY_KEYSTORE_ENV_VAR).as_deref() == Ok("1");
    if on {
        tracing::warn!(
            "{TEST_MEMORY_KEYSTORE_ENV_VAR}=1 (set by .cargo/config.toml for tests): using an \
             in-memory keystore, every key is reminted at the next start; pass --key-store to \
             override, and never run this daemon as your real one"
        );
    }
    on
}

/// Builds this process's one shared [`DeviceIdentity`] (ADR 0021), prompting once for a
/// `file`-keystore passphrase. `--key-store` omitted (`defaulted = true`) now means `auto` (task
/// `relay-id-keystore`): try the OS keychain, falling back to memory with a warning instead of
/// refusing to start when there is none. An explicit `auto|os|file` is unchanged, refusal included.
pub(crate) fn build_identity(
    args: &Args,
    state_dir: &Path,
    clock: &dyn Clock,
) -> Result<BuiltIdentity, Box<dyn std::error::Error>> {
    if test_seam_requested(args) {
        return Ok(BuiltIdentity {
            identity: DeviceIdentity::open_in_memory(state_dir, clock)?,
            sync_allowed: sync_allowed_for("memory", true),
        });
    }
    let defaulted = args.key_store_mode.is_none();
    let key_store_mode = args.key_store_mode.unwrap_or(KeyStoreMode::Auto);
    let file_passphrase = if key_store_mode == KeyStoreMode::File {
        Some(prompt_file_passphrase()?)
    } else {
        None
    };
    let identity =
        DeviceIdentity::open(state_dir, clock, key_store_mode, file_passphrase, defaulted)?;
    let sync_allowed = sync_allowed_for(identity.key_store_backend_name(), false);
    if !sync_allowed {
        tracing::warn!(
            "relay and LAN sync are off for this run: the keystore is in memory, so the device's \
             static, signing and group keys would not survive a restart and every paired peer \
             would have to re-pair; fix the OS keychain or pass --key-store file"
        );
    }
    Ok(BuiltIdentity {
        identity,
        sync_allowed,
    })
}

#[cfg(test)]
mod tests {
    use super::sync_allowed_for;

    #[test]
    fn only_the_fallback_memory_keystore_turns_sync_off() {
        assert!(sync_allowed_for("os", false));
        assert!(sync_allowed_for("file", false));
        assert!(!sync_allowed_for("memory", false), "the auto fallback");
        assert!(
            sync_allowed_for("memory", true),
            "the test seam pairs and never restarts"
        );
    }
}
