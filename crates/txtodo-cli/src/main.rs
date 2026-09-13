//! The txtodo CLI: todo.sh-compatible commands over todo.txt (plan M2, direct-file mode).
#![forbid(unsafe_code)]
#![allow(clippy::print_stdout, clippy::print_stderr)] // the CLI is the output path (plan §0)

mod client;
mod clock;
mod commands;
mod config;
mod daemon_mode;
mod error;
mod json;
mod store;

use clap::{Parser, Subcommand};
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

#[derive(Debug, Subcommand)]
enum Command {
    /// Add a task: today's date after the priority, then an `id:` tag.
    #[command(visible_alias = "a")]
    Add {
        /// The task; several words are joined with spaces.
        #[arg(required = true, num_args = 1..)]
        text: Vec<String>,
    },
    /// Add several tasks, one per line of TEXT.
    Addm {
        /// The tasks; each line becomes one task.
        #[arg(required = true, num_args = 1..)]
        text: Vec<String>,
    },
    /// Add text to the end of a task.
    #[command(visible_alias = "app")]
    Append {
        /// Line number.
        item: String,
        /// Text to append; several words are joined with spaces.
        #[arg(required = true, num_args = 1..)]
        text: Vec<String>,
    },
    /// Move completed lines to done.txt and drop blank lines.
    Archive,
    /// Remove a task's priority.
    #[command(visible_alias = "dp")]
    Depri {
        /// Line numbers, comma or space separated.
        #[arg(required = true, num_args = 1..)]
        items: Vec<String>,
    },
    /// Blank every later repeat of an identical line.
    Deduplicate,
    /// Delete a task (its line stays blank), or remove TERM from it.
    #[command(visible_alias = "rm")]
    Del {
        /// Line number.
        item: String,
        /// Text to remove from the line instead of deleting it.
        term: Option<String>,
    },
    /// Mark tasks done: `x`, today's date, priority kept as `pri:`; then archive.
    Do {
        /// Line numbers, comma or space separated.
        #[arg(required = true, num_args = 1..)]
        items: Vec<String>,
    },
    /// Print the resolved paths and config.
    Env,
    /// Canonicalise quirks in todo.txt (the only command that rewrites untouched lines).
    Fmt,
    /// Report quirks and file hygiene in todo.txt.
    Lint,
    /// List tasks matching every TERM (`-term` excludes), sorted.
    #[command(visible_alias = "ls")]
    List {
        /// Search terms; `-term` excludes.
        #[arg(allow_hyphen_values = true)]
        terms: Vec<String>,
    },
    /// List tasks from todo.txt and done.txt.
    #[command(visible_alias = "lsa")]
    Listall {
        /// Search terms; `-term` excludes.
        #[arg(allow_hyphen_values = true)]
        terms: Vec<String>,
    },
    /// List tasks with a priority, optionally only `A` or a range `A-C`.
    #[command(visible_alias = "lsp")]
    Listpri {
        /// An optional priority or range, then search terms.
        #[arg(allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// List the projects (`+word`) of matching tasks.
    #[command(visible_alias = "lsprj")]
    Listproj {
        /// Search terms; `-term` excludes.
        #[arg(allow_hyphen_values = true)]
        terms: Vec<String>,
    },
    /// List the contexts (`@word`) of matching tasks.
    #[command(visible_alias = "lsc")]
    Listcon {
        /// Search terms; `-term` excludes.
        #[arg(allow_hyphen_values = true)]
        terms: Vec<String>,
    },
    /// Add text after a task's priority and date.
    #[command(visible_alias = "prep")]
    Prepend {
        /// Line number.
        item: String,
        /// Text to prepend; several words are joined with spaces.
        #[arg(required = true, num_args = 1..)]
        text: Vec<String>,
    },
    /// Move a task to another file in the todo directory (its line stays blank).
    #[command(visible_alias = "mv")]
    Move {
        /// Line number.
        item: String,
        /// Destination file name.
        dest: String,
        /// Source file name (default todo.txt).
        src: Option<String>,
    },
    /// Set task priorities: ITEM# PRIORITY pairs, A to Z.
    #[command(visible_alias = "p")]
    Pri {
        /// `ITEM# PRIORITY` pairs.
        #[arg(required = true, num_args = 2..)]
        args: Vec<String>,
    },
    /// Archive, then record the task and done counts in report.txt.
    Report,
    /// Replace a task's text, keeping its priority and date.
    Replace {
        /// Line number.
        item: String,
        /// The new text; several words are joined with spaces.
        #[arg(required = true, num_args = 1..)]
        text: Vec<String>,
    },
    /// List the `.txt` files in the todo directory, or the tasks in FILE.
    #[command(visible_alias = "lf")]
    Listfile {
        /// A file name (looked up in the todo directory) then search terms.
        #[arg(allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Show the op log, newest first (daemon mode).
    Log {
        /// Only this document (workspace-relative), e.g. done.txt.
        #[arg(long)]
        file: Option<String>,
        /// How many ops.
        #[arg(short = 'n', long)]
        n: Option<u32>,
    },
    /// Who last touched each field of a task (daemon mode).
    Blame {
        /// Line number.
        item: String,
    },
    /// Undo the newest ops (daemon mode).
    Undo {
        /// How many ops to invert.
        #[arg(long, default_value_t = 1)]
        steps: u32,
    },
    /// Render a document as it was at a local date-time (daemon mode).
    Checkout {
        /// YYYY-MM-DDTHH:MM[:SS] in the local zone.
        at: String,
        /// Print to stdout instead of a temp file.
        #[arg(long)]
        stdout: bool,
        /// Which document.
        #[arg(long, default_value = "todo.txt")]
        file: String,
    },
    /// Open needs_review flags and resolve them (daemon mode): `list` (default) or `resolve`.
    Conflicts {
        #[command(subcommand)]
        action: Option<commands::conflicts::Action>,
    },
    /// Devices paired into this workspace's sync group (daemon mode): `list` (default) or
    /// `remove <id>`.
    Device {
        #[command(subcommand)]
        action: Option<commands::device::Action>,
    },
    /// Check socket, watcher, files, clock and config; exit 1 on any failure.
    Doctor {
        /// Also print the daemon's recent JSON log.
        #[arg(long)]
        verbose: bool,
    },
    /// Manage the txtodod service for this workspace (launchd on macOS, systemd --user on Linux).
    Daemon {
        /// What to do.
        action: commands::service::Action,
        /// Overwrite an existing service file on install.
        #[arg(long)]
        force: bool,
    },
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
        Command::Device { action } => commands::device::run(daemon, action.as_ref(), ctx.json),
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
        | Command::Device { .. } => Err(CliError::Message(format!(
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
