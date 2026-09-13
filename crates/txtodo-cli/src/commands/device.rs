//! `txtodo device list` and `txtodo device remove <id>` (plan M4 tasks/sync-device-remove).
//! Both need the daemon: devices live in its `devices` table, never in the synced files.

use crate::client::Daemon;
use crate::{CliError, json};
use clap::Subcommand;
use std::io::Write;
use txtodo_proto::v1 as pb;

/// The `txtodo device` subcommands.
#[derive(Debug, Subcommand)]
pub enum Action {
    /// id, name, last seen, key epoch, whether it is this device, clock skew.
    #[command(visible_alias = "ls")]
    List,
    /// Removes a device and rotates the group key to the remaining devices. Refuses removing
    /// this device itself or the last device; requires confirmation naming the device unless
    /// `--yes` is given (for scripts).
    #[command(visible_alias = "rm")]
    Remove {
        /// The device's id (ULID text, from `device list`).
        id: String,
        /// Skip the confirmation prompt.
        #[arg(long)]
        yes: bool,
    },
}

/// Entry: `txtodo device` with no subcommand is `list` (matches `conflicts`' own convenience).
pub fn run(daemon: &mut Daemon, action: Option<&Action>, as_json: bool) -> Result<(), CliError> {
    match action {
        None | Some(Action::List) => run_list(daemon, as_json),
        Some(Action::Remove { id, yes }) => run_remove(daemon, id, *yes, as_json),
    }
}

/// The lowercase word for a `SkewStatus`; an out-of-range wire value reads as `unknown` rather
/// than panicking (a hostile or newer-daemon value is external input, never asserted on).
fn skew_word(status: i32) -> &'static str {
    match pb::SkewStatus::try_from(status).unwrap_or(pb::SkewStatus::Unspecified) {
        pb::SkewStatus::Ok => "ok",
        pb::SkewStatus::Behind => "behind",
        pb::SkewStatus::Ahead => "ahead",
        pb::SkewStatus::Unknown | pb::SkewStatus::Unspecified => "unknown",
    }
}

fn device_json(d: &pb::Device) -> String {
    format!(
        r#"{{"id":{},"name":{},"is_self":{},"removed":{},"key_epoch":{},"paired_at_ms":{},"last_seen_ms":{},"skew":{}}}"#,
        json::str(&d.id),
        json::str(&d.name),
        d.is_self,
        d.removed,
        d.key_epoch,
        d.paired_at_ms,
        d.last_seen_ms,
        json::str(skew_word(d.skew_status))
    )
}

fn device_text(d: &pb::Device) -> String {
    let name = if d.name.is_empty() {
        "(unnamed)"
    } else {
        d.name.as_str()
    };
    let marker = if d.is_self { " (this device)" } else { "" };
    let removed = if d.removed { " [removed]" } else { "" };
    let last_seen = if d.last_seen_ms == 0 {
        "never".to_owned()
    } else {
        format!("{} ms", d.last_seen_ms)
    };
    format!(
        "{}  {name}{marker}{removed}  epoch={}  last_seen={last_seen}  clock={}",
        d.id,
        d.key_epoch,
        skew_word(d.skew_status)
    )
}

/// `device list`: one row per known device, oldest paired first (the daemon's own order).
pub fn run_list(daemon: &mut Daemon, as_json: bool) -> Result<(), CliError> {
    let devices = daemon.device_list()?;
    if devices.is_empty() {
        if !as_json {
            println!("TODO: no paired devices.");
        }
        return Ok(());
    }
    for d in &devices {
        println!(
            "{}",
            if as_json {
                device_json(d)
            } else {
                device_text(d)
            }
        );
    }
    Ok(())
}

/// `device remove <id> [--yes]`: confirms by making the human type the id back (CLAUDE.md:
/// outward-facing and hard to reverse), then rotates the group key on the daemon.
pub fn run_remove(daemon: &mut Daemon, id: &str, yes: bool, as_json: bool) -> Result<(), CliError> {
    if !yes && !confirm(daemon, id)? {
        return Err(CliError::Message(
            "txtodo: removal not confirmed; nothing changed.".to_owned(),
        ));
    }
    let rep = daemon.device_remove(id)?;
    if !rep.removed {
        return Err(CliError::Message(format!("txtodo: {}", rep.message)));
    }
    if as_json {
        println!(
            r#"{{"removed":{},"already_removed":{},"rotated_to_epoch":{},"message":{}}}"#,
            rep.removed,
            rep.already_removed,
            rep.rotated_to_epoch,
            json::str(&rep.message)
        );
    } else {
        println!("TODO: {}", rep.message);
    }
    Ok(())
}

/// Prompts the human to type `id` back, naming the device by label when known. `false` means the
/// human typed something else (or nothing) — the caller refuses the removal, not a failure.
fn confirm(daemon: &mut Daemon, id: &str) -> Result<bool, CliError> {
    let devices = daemon.device_list()?;
    let label = devices
        .iter()
        .find(|d| d.id == id)
        .map(|d| {
            if d.name.is_empty() {
                d.id.clone()
            } else {
                format!("{} ({})", d.name, d.id)
            }
        })
        .unwrap_or_else(|| id.to_owned());
    print!(
        "Remove device {label}? This does not un-share history it already synced. Type its id to confirm: "
    );
    std::io::stdout().flush().ok();
    let mut line = String::new();
    std::io::stdin()
        .read_line(&mut line)
        .map_err(CliError::Io)?;
    Ok(line.trim() == id)
}
