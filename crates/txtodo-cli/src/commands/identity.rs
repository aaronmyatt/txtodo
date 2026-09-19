//! `txtodo identity migrate [--dry-run] [--yes]` (tasks/sidecar-migrate-tagged, ADR 0019).
//! Converts a workspace that stamps `id:` tags into its files to Sidecar identity: the tags come
//! out of every line, each task keeps its identity and history in the daemon's own store.

use crate::client::Daemon;
use crate::{CliError, json};
use clap::Subcommand;
use std::io::Write;
use txtodo_proto::v1 as pb;

/// The `txtodo identity` subcommands.
#[derive(Debug, Subcommand)]
pub enum Action {
    /// Removes every `id:` tag from this workspace's todo files and keeps each task's identity
    /// and history. Shows what it would change and asks first; rewrites every tagged line, so
    /// review the diff and commit it afterwards.
    Migrate {
        /// Only report what would change.
        #[arg(long)]
        dry_run: bool,
        /// Skip the confirmation prompt.
        #[arg(long)]
        yes: bool,
    },
}

/// Entry point for `txtodo identity`.
pub fn run(daemon: &mut Daemon, action: &Action, as_json: bool) -> Result<(), CliError> {
    match action {
        Action::Migrate { dry_run, yes } => run_migrate(daemon, *dry_run, *yes, as_json),
    }
}

fn summary_json(r: &pb::MigrateIdentityResponse, dry_run: bool) -> String {
    format!(
        r#"{{"dry_run":{},"was_tagged":{},"files":{},"tasks":{},"stripped":{},"renumbered":{},"failures":{}}}"#,
        dry_run,
        r.was_tagged,
        r.files,
        r.tasks,
        r.stripped,
        r.renumbered,
        json::strs(r.failures.iter().map(String::as_str))
    )
}

fn run_migrate(
    daemon: &mut Daemon,
    dry_run: bool,
    yes: bool,
    as_json: bool,
) -> Result<(), CliError> {
    let plan = daemon.migrate_identity(true)?;
    if dry_run {
        return report(&plan, true, as_json);
    }
    if plan.stripped == 0 && !plan.was_tagged {
        println!("Already sidecar: no id: tags left to remove.");
        return Ok(());
    }
    if !yes && !confirm(&plan)? {
        return Err(CliError::Message(
            "txtodo: migration not confirmed; nothing changed.".to_owned(),
        ));
    }
    let done = daemon.migrate_identity(false)?;
    report(&done, false, as_json)?;
    if done.failures.is_empty() {
        Ok(())
    } else {
        Err(CliError::Message(
            "txtodo: some documents were not migrated; run the command again to retry them."
                .to_owned(),
        ))
    }
}

fn report(r: &pb::MigrateIdentityResponse, dry_run: bool, as_json: bool) -> Result<(), CliError> {
    if as_json {
        println!("{}", summary_json(r, dry_run));
        return Ok(());
    }
    let verb = if dry_run { "Would remove" } else { "Removed" };
    println!(
        "{verb} {} id: tags from {} task lines in {} files.",
        r.stripped, r.tasks, r.files
    );
    if r.renumbered > 0 {
        println!(
            "{} lines repeated another line's id and {} a new one (the first keeps its history).",
            r.renumbered,
            if dry_run { "would get" } else { "got" }
        );
    }
    for failure in &r.failures {
        eprintln!("txtodo: failed: {failure}");
    }
    Ok(())
}

/// The human types `migrate` back: the change rewrites files and is not undone by `txtodo undo`
/// alone (the tags come back only by re-tagging), so a stray Enter must not run it.
fn confirm(plan: &pb::MigrateIdentityResponse) -> Result<bool, CliError> {
    print!(
        "This removes {} id: tags from {} files. Task history is kept. Type migrate to confirm: ",
        plan.stripped, plan.files
    );
    std::io::stdout().flush().ok();
    let mut line = String::new();
    std::io::stdin()
        .read_line(&mut line)
        .map_err(CliError::Io)?;
    Ok(line.trim() == "migrate")
}
