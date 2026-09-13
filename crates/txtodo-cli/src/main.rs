//! The txtodo CLI: todo.sh-compatible commands over todo.txt (plan M2, direct-file mode).
#![forbid(unsafe_code)]
#![allow(clippy::print_stdout, clippy::print_stderr)] // the CLI is the output path (plan §0)

mod cli_command;
mod client;
mod clock;
mod commands;
mod config;
mod daemon_mode;
mod error;
mod json;
mod store;

use clap::Parser;
use cli_command::Command;
use config::{Config, Env, Paths};
use error::CliError;
use std::process::ExitCode;
use txtodo_core::Date;

/// todo.sh-compatible todo.txt tool. Commands and aliases match todo.sh; line numbers are the ids.
#[derive(Debug, Parser)]
#[command(name = "txtodo", version, about)]
struct Cli {
    /// Todo directory (overrides $TXTODO_TODO_DIR and config `todo_dir`).
    #[arg(long, global = true, value_name = "DIR")]
    dir: Option<String>,
    /// Emit one JSON object per line on listing commands.
    #[arg(long, global = true)]
    json: bool,
    /// Do not stamp `id:` on added tasks (overrides config `id_tags`).
    #[arg(long, global = true)]
    no_id: bool,
    /// Do not archive after `do` (todo.sh -A).
    #[arg(short = 'A', long, global = true)]
    no_archive: bool,
    /// Ignore a running daemon and edit the files directly (M2 behaviour).
    #[arg(long, global = true)]
    no_daemon: bool,
    #[command(subcommand)]
    command: Command,
}

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
    let paths = config::resolve(&env, cli.dir.as_deref(), &config, config_file);
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
        Command::Open { item } => commands::refdir::run_open(ctx, daemon, item),
        Command::Notes { item } => commands::refdir::run_notes(daemon, item),
        Command::Sub { item, cmd } => commands::refdir::run_sub(ctx, daemon, item, cmd),
        Command::Prune { orphans, yes } => {
            commands::refdir::run_prune(daemon, *orphans, *yes, ctx.json)
        }
        // Every todo.sh command, present and future, goes through the scratch adapter by design.
        todo_sh => daemon_mode::run_via_daemon(ctx, daemon, |scratch| dispatch(scratch, todo_sh)),
    }
}

/// Direct-file mode (M2): one arm per command.
fn dispatch(ctx: &Ctx, command: &Command) -> Result<(), CliError> {
    match command {
        Command::Doctor { .. } | Command::Daemon { .. } => {
            unreachable!("doctor and daemon are handled before mode selection")
        }
        Command::Log { .. }
        | Command::Blame { .. }
        | Command::Undo { .. }
        | Command::Checkout { .. }
        | Command::Conflicts { .. }
        | Command::Open { .. }
        | Command::Notes { .. }
        | Command::Sub { .. }
        | Command::Prune { .. } => Err(CliError::Message(format!(
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
            print_env(ctx);
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

/// `txtodo env`: one `key=value` per line, or one JSON object.
fn print_env(ctx: &Ctx) {
    let schemes = ctx.config.url_schemes();
    let exists = if ctx.paths.config.exists() {
        ""
    } else {
        " (missing)"
    };
    if ctx.json {
        let object = format!(
            r#"{{"todo_dir":{},"todo_file":{},"done_file":{},"report_file":{},"config_file":{},"config_exists":{},"id_tags":{},"url_schemes":[{}]}}"#,
            json::str(&ctx.paths.dir.to_string_lossy()),
            json::str(&ctx.paths.todo.to_string_lossy()),
            json::str(&ctx.paths.done.to_string_lossy()),
            json::str(&ctx.paths.report.to_string_lossy()),
            json::str(&ctx.paths.config.to_string_lossy()),
            exists.is_empty(),
            ctx.config.id_tags(),
            schemes
                .iter()
                .map(|s| json::str(s))
                .collect::<Vec<_>>()
                .join(",")
        );
        debug_assert!(
            object.starts_with('{') && object.ends_with('}'),
            "one object"
        );
        println!("{object}");
        return;
    }
    println!("todo_dir={}", ctx.paths.dir.display());
    println!("todo_file={}", ctx.paths.todo.display());
    println!("done_file={}", ctx.paths.done.display());
    println!("report_file={}", ctx.paths.report.display());
    println!("config_file={}{exists}", ctx.paths.config.display());
    println!("id_tags={}", ctx.config.id_tags());
    println!("url_schemes={}", schemes.join(","));
}
