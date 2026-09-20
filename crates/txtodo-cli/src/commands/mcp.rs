//! `txtodo mcp --stdio | --http [--token <t>]` (mcp-transports notes.md). There is no `--lan` and
//! no bind option: the MCP server is reachable from this device only (ADR 0028). This crate may
//! not depend on `txtodo-mcp` (`budgets.json`'s `allowedDeps` grants that edge only to
//! `txtodo-daemon`), so this command is a thin process launcher: it execs the sibling
//! `txtodo-mcp` binary (built from the crate of the same name), the same "binary beside this one,
//! else PATH" pattern `commands::service::txtodod_path` already uses for `txtodod`. That binary
//! owns the real `rmcp` server and the gRPC client dialing the daemon; this command only resolves
//! it, forwards flags, and inherits stdio so `--stdio`'s JSON-RPC framing passes through untouched.

use crate::{CliError, Ctx};
use std::path::PathBuf;
use std::process::Command;

const USAGE: &str = "mcp --stdio | --http [--token TOKEN]";

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

/// The flags handed to `txtodo-mcp`, or a usage error: exactly one of `--stdio` and `--http`. Pure,
/// so the tests need no `Ctx` and spawn nothing. `--token` names the agent principal on a mutation;
/// it passes through as it is.
fn mcp_args(stdio: bool, http: bool, token: Option<&str>) -> Result<Vec<String>, CliError> {
    if stdio == http {
        return Err(CliError::Usage(USAGE));
    }
    let mut args = vec![if stdio { "--stdio" } else { "--http" }.to_owned()];
    if let Some(t) = token {
        args.extend(["--token".to_owned(), t.to_owned()]);
    }
    Ok(args)
}

/// `txtodo mcp`'s entry point.
pub fn run(ctx: &Ctx, stdio: bool, http: bool, token: Option<&str>) -> Result<(), CliError> {
    let args = mcp_args(stdio, http, token)?;
    let mut cmd = Command::new(txtodo_mcp_path());
    cmd.arg("--dir").arg(&ctx.paths.dir).args(args);
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

    #[test]
    fn exactly_one_of_stdio_http_is_required() {
        assert!(matches!(
            mcp_args(true, true, None),
            Err(CliError::Usage(_))
        ));
        assert!(matches!(
            mcp_args(false, false, None),
            Err(CliError::Usage(_))
        ));
        assert!(mcp_args(true, false, None).is_ok());
    }

    #[test]
    fn the_transport_and_the_token_pass_through_and_nothing_else_does() {
        let stdio = mcp_args(true, false, None).unwrap_or_default();
        assert_eq!(stdio, ["--stdio"]);
        let http = mcp_args(false, true, Some("01TOKEN")).unwrap_or_default();
        assert_eq!(http, ["--http", "--token", "01TOKEN"]);
        // No flag widens the bind: the MCP server is loopback only (ADR 0028).
        assert!(!http.iter().any(|a| a == "--lan" || a.contains("bind")));
    }
}
