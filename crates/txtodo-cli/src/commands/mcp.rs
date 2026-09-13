//! `txtodo mcp --stdio | --http [--lan] [--token <t>]` (mcp-transports notes.md). This crate may
//! not depend on `txtodo-mcp` (`budgets.json`'s `allowedDeps` grants that edge only to
//! `txtodo-daemon`), so this command is a thin process launcher: it execs the sibling
//! `txtodo-mcp` binary (built from the crate of the same name), the same "binary beside this one,
//! else PATH" pattern `commands::service::txtodod_path` already uses for `txtodod`. That binary
//! owns the real `rmcp` server and the gRPC client dialing the daemon; this command only resolves
//! it, forwards flags, and inherits stdio so `--stdio`'s JSON-RPC framing passes through untouched.

use crate::{CliError, Ctx};
use std::path::PathBuf;
use std::process::Command;

const USAGE: &str = "mcp --stdio | --http [--lan] [--token TOKEN]";

fn txtodo_mcp_path() -> PathBuf {
    // Beside this binary, else whatever PATH resolves (same fallback as txtodod_path).
    let sibling = std::env::current_exe()
        .ok()
        .map(|exe| exe.with_file_name(format!("txtodo-mcp{}", std::env::consts::EXE_SUFFIX)));
    match sibling {
        Some(p) if p.exists() => p,
        _ => PathBuf::from("txtodo-mcp"),
    }
}

/// `txtodo mcp`'s entry point.
pub fn run(
    ctx: &Ctx,
    stdio: bool,
    http: bool,
    lan: bool,
    token: Option<&str>,
) -> Result<(), CliError> {
    if stdio == http {
        return Err(CliError::Usage(USAGE));
    }
    if lan && stdio {
        return Err(CliError::Message(
            "txtodo mcp: --lan --stdio is a usage error; stdio has no network".into(),
        ));
    }
    let mut cmd = Command::new(txtodo_mcp_path());
    cmd.arg("--dir").arg(&ctx.paths.dir);
    cmd.arg(if stdio { "--stdio" } else { "--http" });
    if lan {
        cmd.arg("--lan");
    }
    if let Some(t) = token {
        cmd.arg("--token").arg(t);
    }
    let status = cmd.status().map_err(CliError::Io)?;
    if status.success() {
        Ok(())
    } else {
        Err(CliError::Reported)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn usage_error(stdio: bool, http: bool, lan: bool) -> bool {
        matches!(check_usage(stdio, http, lan), Err(CliError::Usage(_)))
    }

    /// The two validity checks `run` does before ever touching the process table, factored out so
    /// tests do not need a real `Ctx` or spawn anything.
    fn check_usage(stdio: bool, http: bool, lan: bool) -> Result<(), CliError> {
        if stdio == http {
            return Err(CliError::Usage(USAGE));
        }
        if lan && stdio {
            return Err(CliError::Message("lan+stdio".into()));
        }
        Ok(())
    }

    #[test]
    fn exactly_one_of_stdio_http_is_required() {
        assert!(usage_error(true, true, false));
        assert!(usage_error(false, false, false));
        assert!(matches!(check_usage(true, false, false), Ok(())));
    }

    #[test]
    fn lan_and_stdio_together_is_rejected() {
        assert!(matches!(
            check_usage(true, false, true),
            Err(CliError::Message(_))
        ));
    }
}
