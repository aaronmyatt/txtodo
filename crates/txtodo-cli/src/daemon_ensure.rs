//! Optimistic ensure-then-retry wiring for `main.rs`'s `run()` (task `daemon-always-available`,
//! item 4): when the first `client::select` call finds neither a per-dir nor a global socket for
//! a command that needs one, this tries to spawn the global daemon before falling back to the
//! honest `needs_daemon_err()`. Split out of `main.rs` purely for its own file-length budget, the
//! same pattern `apps/desktop/src-tauri/src/commands.rs` used for `commands_ui_log.rs`.

use crate::cli::Command;
use crate::config::{self, Env};
use crate::{CliError, Ctx, client, dispatch, dispatch_daemon};

/// Every `Command` variant that only daemon mode can serve — the single source of truth for that
/// variant list, reused by `main.rs::dispatch_inner`'s own `Err(needs_daemon_err())` arm so the
/// names are written once, not twice.
pub fn needs_daemon(command: &Command) -> bool {
    matches!(
        command,
        Command::Log { .. }
            | Command::Blame { .. }
            | Command::Undo { .. }
            | Command::Checkout { .. }
            | Command::Conflicts { .. }
            | Command::Pair { .. }
            | Command::Open { .. }
            | Command::Notes { .. }
            | Command::Sub { .. }
            | Command::Prune { .. }
            | Command::Device { .. }
            | Command::Workspace { .. }
            | Command::Bundle { .. }
    )
}

/// `client::select`'s first call found neither socket for a command that needs one. Tries
/// `txtodo_daemon_launch::ensure_daemon` against the global socket, then retries `select` once.
/// `ensure_daemon` is async, so this builds a throwaway current-thread runtime just for the call —
/// the same pattern `client::Daemon::connect` already uses to bridge sync CLI code into async
/// gRPC/socket work. Any failure here (no runtime, no `txtodod` on `$PATH`, spawn/timeout) is
/// swallowed: the retry's `select()` will simply see `Mode::Direct` again and fall through to the
/// same honest `needs_daemon_err()` the user would have seen without this wiring at all — never a
/// different or more confusing error than today's.
pub fn ensure_daemon_then_dispatch(
    ctx: &Ctx,
    command: &Command,
    env: &Env,
) -> Result<(), CliError> {
    if !txtodo_daemon_launch::autostart_disabled() {
        let cfg = txtodo_daemon_launch::LaunchConfig::new(config::global_socket_path(env));
        if let Ok(rt) = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            let _ = rt.block_on(txtodo_daemon_launch::ensure_daemon(&cfg));
        }
    }
    match client::select(&ctx.paths.dir, false, env) {
        Ok(client::Mode::Daemon(mut daemon)) => dispatch_daemon(ctx, &mut daemon, command),
        _ => dispatch(ctx, command),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn needs_daemon_is_true_for_the_simple_history_variants() {
        assert!(needs_daemon(&Command::Log {
            file: None,
            n: None
        }));
        assert!(needs_daemon(&Command::Blame {
            item: "1".to_owned()
        }));
        assert!(needs_daemon(&Command::Undo { steps: 1 }));
        assert!(needs_daemon(&Command::Checkout {
            at: "2026-01-01T00:00".to_owned(),
            stdout: false,
            file: "todo.txt".to_owned()
        }));
    }

    #[test]
    fn needs_daemon_is_true_for_the_refdir_and_pairing_variants() {
        assert!(needs_daemon(&Command::Conflicts { action: None }));
        assert!(needs_daemon(&Command::Pair { code: None }));
        assert!(needs_daemon(&Command::Open {
            item: "1".to_owned()
        }));
        assert!(needs_daemon(&Command::Notes {
            item: "1".to_owned()
        }));
        assert!(needs_daemon(&Command::Sub {
            item: "1".to_owned(),
            cmd: vec!["ls".to_owned()]
        }));
        assert!(needs_daemon(&Command::Prune {
            orphans: true,
            yes: false
        }));
    }

    #[test]
    fn needs_daemon_is_true_for_the_registry_and_bundle_variants() {
        assert!(needs_daemon(&Command::Device { action: None }));
        assert!(needs_daemon(&Command::Workspace { action: None }));
        assert!(needs_daemon(&Command::Bundle {
            action: crate::bundle::Action::Import {
                file: "bundle.txtodo".to_owned(),
                passphrase_file: None
            }
        }));
    }

    #[test]
    fn needs_daemon_is_false_for_file_only_and_special_cased_variants() {
        assert!(!needs_daemon(&Command::Env));
        assert!(!needs_daemon(&Command::Archive));
        assert!(!needs_daemon(&Command::Add {
            text: vec!["buy milk".to_owned()]
        }));
        assert!(!needs_daemon(&Command::Doctor { verbose: false }));
        assert!(!needs_daemon(&Command::Daemon {
            action: crate::commands::service::Action::Status,
            force: false
        }));
        assert!(!needs_daemon(&Command::Mcp {
            stdio: true,
            http: false,
            lan: false,
            token: None
        }));
    }
}
