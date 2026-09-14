//! The txtodo CLI: todo.sh-compatible commands over todo.txt (plan M2, direct-file mode).
#![forbid(unsafe_code)]
#![allow(clippy::print_stdout, clippy::print_stderr)] // the CLI is the output path (plan §0)

mod bundle;
mod cli;
mod client;
mod client_pairing;
mod clock;
mod commands;
mod config;
mod daemon_mode;
mod error;
mod json;
mod store;

use clap::Parser;
use cli::{Cli, Command};
use config::{Config, Env, Paths};
use error::CliError;
use std::process::ExitCode;
use txtodo_core::Date;

/// Everything a command needs: where the files are and what the config says.
struct Ctx {
    paths: Paths,
    config: Config,
    json: bool,
    /// Stamp `id:` on add (config `id_tags` and not `--no-id`).
    ids: bool,
    /// `do` archives afterwards (todo.sh default; `-A` turns it off).
    auto_archive: bool,
    /// The local calendar date at startup.
    today: Date,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(&cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(CliError::Reported) => ExitCode::FAILURE,
        Err(e) => {
            eprintln!("txtodo: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: &Cli) -> Result<(), CliError> {
    let env = Env::from_process().map_err(CliError::Io)?;
    let config_file = config::config_path(&env);
    let config = Config::load(&config_file).map_err(CliError::Config)?;
    let paths = config::resolve(
        &env,
        config::ResolveFlags {
            dir: cli.dir.as_deref(),
            sync_dir: cli.sync_dir.as_deref(),
            relay: cli.relay.as_deref(),
        },
        &config,
        config_file,
    );
    debug_assert!(
        paths.todo.ends_with("todo.txt"),
        "resolve names the todo file"
    );
    let ctx = Ctx {
        ids: config.id_tags() && !cli.no_id,
        auto_archive: !cli.no_archive,
        today: clock::today_local(),
        paths,
        config,
        json: cli.json,
    };
    match &cli.command {
        Command::Doctor { verbose } => return commands::doctor::run(&ctx, *verbose),
        Command::Daemon { action, force } => return commands::service::run(&ctx, *action, *force),
        Command::Mcp {
            stdio,
            http,
            lan,
            token,
        } => return commands::mcp::run(&ctx, *stdio, *http, *lan, token.as_deref()),
        _ => {}
    }
    match client::select(&ctx.paths.dir, cli.no_daemon)? {
        client::Mode::Direct => dispatch(&ctx, &cli.command),
        client::Mode::Daemon(mut daemon) => dispatch_daemon(&ctx, &mut daemon, &cli.command),
    }
}

/// Daemon mode: history commands talk to the daemon directly; every todo.sh command runs against a
/// scratch copy and its diff is sent as mutations (`daemon_mode`). `env` reports the real paths.
fn dispatch_daemon(
    ctx: &Ctx,
    daemon: &mut client::Daemon,
    command: &Command,
) -> Result<(), CliError> {
    match command {
        Command::Env => dispatch(ctx, command),
        Command::Log { file, n } => {
            commands::history::run_log(daemon, file.as_deref(), *n, ctx.json)
        }
        Command::Blame { item } => commands::history::run_blame(daemon, item, ctx.json),
        Command::Undo { steps } => commands::history::run_undo(daemon, *steps, ctx.json),
        Command::Checkout { at, stdout, file } => {
            commands::history::run_checkout(daemon, at, file, *stdout)
        }
        Command::Conflicts { action } => {
            commands::conflicts::run(daemon, action.as_ref(), ctx.json)
        }
        Command::Pair { code } => commands::pair::run(ctx, daemon, code.as_deref()),
        Command::Open { item } => commands::refdir::run_open(ctx, daemon, item),
        Command::Notes { item } => commands::refdir::run_notes(daemon, item),
        Command::Sub { item, cmd } => commands::refdir::run_sub(ctx, daemon, item, cmd),
        Command::Prune { orphans, yes } => {
            commands::refdir::run_prune(daemon, *orphans, *yes, ctx.json)
        }
        Command::Device { action } => commands::device::run(daemon, action.as_ref(), ctx.json),
        Command::Bundle { action } => bundle::run(daemon, action),
        // Every todo.sh command, present and future, goes through the scratch adapter by design.
        todo_sh => daemon_mode::run_via_daemon(ctx, daemon, |scratch| dispatch(scratch, todo_sh)),
    }
}

/// Direct-file mode (M2): one arm per command.
fn dispatch(ctx: &Ctx, command: &Command) -> Result<(), CliError> {
    match command {
        Command::Doctor { .. } | Command::Daemon { .. } | Command::Mcp { .. } => {
            unreachable!("doctor, daemon and mcp are handled before mode selection")
        }
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
        | Command::Bundle { .. } => Err(CliError::Message(format!(
            "txtodo: {}",
            commands::history::NEEDS_DAEMON
        ))),
        Command::Add { text } => commands::add::run(ctx, &text.join(" "), false),
        Command::Addm { text } => commands::add::run(ctx, &text.join(" "), true),
        Command::Append { item, text } => {
            commands::text::run(ctx, commands::text::Kind::Append, item, &text.join(" "))
        }
        Command::Prepend { item, text } => {
            commands::text::run(ctx, commands::text::Kind::Prepend, item, &text.join(" "))
        }
        Command::Replace { item, text } => {
            commands::text::run(ctx, commands::text::Kind::Replace, item, &text.join(" "))
        }
        Command::Archive => commands::archive::run(ctx),
        Command::Depri { items } => commands::edit::run_depri(ctx, items),
        Command::Deduplicate => commands::fileops::run_dedup(ctx),
        Command::Move { item, dest, src } => {
            commands::fileops::run_move(ctx, item, dest, src.as_deref())
        }
        Command::Report => commands::fileops::run_report(ctx, &clock::now_local_iso()),
        Command::Del { item, term } => commands::edit::run_del(ctx, item, term.as_deref()),
        Command::Do { items } => commands::edit::run_do(ctx, items),
        Command::Pri { args } => commands::edit::run_pri(ctx, args),
        Command::Env => {
            commands::env::run(ctx);
            Ok(())
        }
        Command::Fmt => commands::hygiene::run_fmt(ctx),
        Command::Lint => commands::hygiene::run_lint(ctx),
        Command::List { terms } => commands::list::list_file(ctx, &ctx.paths.todo, terms),
        Command::Listall { terms } => commands::list::list_all(ctx, terms),
        Command::Listpri { args } => commands::list::list_pri(ctx, args),
        Command::Listproj { terms } => commands::list::list_words(ctx, '+', terms),
        Command::Listcon { terms } => commands::list::list_words(ctx, '@', terms),
        Command::Listfile { args } => match args.split_first() {
            None => commands::list::list_txt_files(ctx),
            Some((name, terms)) => {
                let path = commands::list::find_file(ctx, name)?;
                commands::list::list_file(ctx, &path, terms)
            }
        },
    }
}
