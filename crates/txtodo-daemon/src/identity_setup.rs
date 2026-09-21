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

/// Builds this process's one shared [`DeviceIdentity`] (ADR 0021), prompting once for a
/// `file`-keystore passphrase. `--key-store` omitted (`defaulted = true`) now means `auto` (task
/// `relay-id-keystore`): try the OS keychain, falling back to memory with a warning instead of
/// refusing to start when there is none. An explicit `auto|os|file` is unchanged, refusal included.
pub(crate) fn build_identity(
    args: &Args,
    state_dir: &Path,
    clock: &dyn Clock,
) -> Result<DeviceIdentity, Box<dyn std::error::Error>> {
    if args.key_store_mode.is_none()
        && std::env::var(TEST_MEMORY_KEYSTORE_ENV_VAR).as_deref() == Ok("1")
    {
        return Ok(DeviceIdentity::open_in_memory(state_dir, clock)?);
    }
    let defaulted = args.key_store_mode.is_none();
    let key_store_mode = args.key_store_mode.unwrap_or(KeyStoreMode::Auto);
    let file_passphrase = if key_store_mode == KeyStoreMode::File {
        Some(prompt_file_passphrase()?)
    } else {
        None
    };
    Ok(DeviceIdentity::open(
        state_dir,
        clock,
        key_store_mode,
        file_passphrase,
        defaulted,
    )?)
}
