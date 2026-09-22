//! The txtodo CLI: todo.sh-compatible commands over todo.txt (plan M2, direct-file mode).
#![forbid(unsafe_code)]
#![allow(clippy::print_stdout, clippy::print_stderr)] // the CLI is the output path (plan §0)

mod archive_plan;
mod base_guard;
mod buildinfo;
mod bundle;
mod cli;
mod client;
mod client_bundle;
mod client_identity;
mod client_pairing;
#[cfg(test)]
mod client_tests;
mod client_workspace;
mod clock;
mod commands;
mod config;
mod daemon_ensure;
mod daemon_mode;
mod error;
mod json;
#[cfg(test)]
mod plan_audit;
mod plan_check;
mod store;

use clap::Parser;
use cli::{Cli, Command};
use config::{Config, Env, Paths};
use error::CliError;
use std::path::Path;
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
        paths.todo.ends_with(&paths.todo_file),
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
    announce_default_workspace(&ctx, &cli.command);
    let _log_guard = init_telemetry(&ctx.paths.dir);
    match &cli.command {
        Command::Doctor { verbose } => return commands::doctor::run(&ctx, *verbose),
        Command::Daemon { action, force } => return commands::service::run(&ctx, *action, *force),
        Command::Mcp { stdio, http, token } => {
            return commands::mcp::run(&ctx, *stdio, *http, token.as_deref());
        }
        Command::Skill { action } => return commands::skill::run(action),
        // A path, not a daemon question: answered even when no daemon is running.
        Command::Workspace {
            action: Some(commands::workspace::Action::Default),
        } => return commands::workspace::run_default(&env, cli.json),
        _ => {}
    }
    if !cli.no_daemon
        && let Some((from, to)) = daemon_ensure::upgrade_running_daemon(&env)
    {
        eprintln!("txtodo: restarted the older daemon {from} with this build's txtodod ({to})");
    }
    match client::select(&ctx.paths.dir, cli.no_daemon, &env)? {
        client::Mode::Direct if daemon_ensure::needs_daemon(&cli.command) && !cli.no_daemon => {
            daemon_ensure::ensure_daemon_then_dispatch(&ctx, &cli.command, &env)
        }
        client::Mode::Direct => dispatch(&ctx, &cli.command),
        client::Mode::Daemon(mut daemon) => dispatch_daemon(&ctx, &mut daemon, &cli.command),
    }
}

/// Says which workspace a command is about to use when it fell back to the default one (task
/// default-workspace): the same command means a different list depending on where you stand, so
/// it must not be silent. Also makes the directory, so direct-file mode has somewhere to write on a
/// machine where no daemon has run yet. Commands about the daemon or workspaces themselves skip it.
fn announce_default_workspace(ctx: &Ctx, command: &Command) {
    if !ctx.paths.default_workspace
        || matches!(
            command,
            Command::Doctor { .. }
                | Command::Daemon { .. }
                | Command::Skill { .. }
                | Command::Workspace { .. }
        )
    {
        return;
    }
    if let Err(e) = std::fs::create_dir_all(&ctx.paths.dir) {
        eprintln!(
            "txtodo: cannot create the default workspace {}: {e}",
            ctx.paths.dir.display()
        );
        return;
    }
    eprintln!(
        "txtodo: no workspace here, using the default workspace ({}); `txtodo workspace default` prints it",
        ctx.paths.dir.display()
    );
}

/// The CLI's own sink matrix (root todo.txt `logging-cli`), distinct from `txtodo-mcp`/`txtodod`'s
/// always-on JSON+stderr pair (`txtodo_telemetry::init` unconditionally builds both): most
/// invocations are one-shot and short-lived, so writing a rolling JSON log file nobody asked for on
/// every `txtodo add` would be noise. Default (no `$TXTODO_LOG`): pretty stderr only, `warn`+, so
/// an ordinary run stays quiet but still surfaces the three converted `commands/edit.rs`
/// diagnostics. `$TXTODO_LOG` set: the shared JSON-rolling-file + pretty-stderr pair, filtered by
/// its value, same as every other txtodo binary.
///
/// Reads `$TXTODO_LOG` directly rather than through `config::Env` (`config.rs`'s module doc: "the
/// process environment ... never read below main"): this is telemetry bootstrap, not CLI business
/// logic, and every other txtodo binary's own `tracing-subscriber::EnvFilter::try_from_env` reads
/// the same variable directly, hidden inside `txtodo_telemetry::init` itself.
fn init_telemetry(dir: &Path) -> Option<txtodo_telemetry::LogGuard> {
    if std::env::var_os(txtodo_telemetry::LOG_FILTER_ENV).is_some() {
        return txtodo_telemetry::init("txtodo", &dir.join(".txtodo/logs")).ok();
    }
    // https://docs.rs/tracing-subscriber/latest/tracing_subscriber/fmt/fn.fmt.html
    let _ = tracing_subscriber::fmt()
        .with_writer(std::io::stderr as fn() -> std::io::Stderr)
        .with_max_level(tracing::Level::WARN)
        .try_init();
    None
}

/// The command's declared name for the `cli.command` span — never `Debug`/`Display` on `Command`
/// itself, which would leak text arguments (`add "buy milk"` etc.) into a span field.
fn command_name(command: &Command) -> &'static str {
    match command {
        Command::Add { .. } => "add",
        Command::Addm { .. } => "addm",
        Command::Append { .. } => "append",
        Command::Archive => "archive",
        Command::Depri { .. } => "depri",
        Command::Deduplicate => "deduplicate",
        Command::Del { .. } => "del",
        Command::Do { .. } => "do",
        Command::Env => "env",
        Command::Fmt => "fmt",
        Command::Lint => "lint",
        Command::List { .. } => "list",
        Command::Listall { .. } => "listall",
        Command::Listpri { .. } => "listpri",
        Command::Listproj { .. } => "listproj",
        Command::Listcon { .. } => "listcon",
        Command::Prepend { .. } => "prepend",
        Command::Move { .. } => "move",
        Command::Pri { .. } => "pri",
        Command::Report => "report",
        Command::Replace { .. } => "replace",
        Command::Listfile { .. } => "listfile",
        Command::Log { .. } => "log",
        Command::Blame { .. } => "blame",
        Command::Undo { .. } => "undo",
        Command::Checkout { .. } => "checkout",
        Command::Conflicts { .. } => "conflicts",
        Command::Device { .. } => "device",
        Command::Identity { .. } => "identity",
        Command::Pair { .. } => "pair",
        Command::Doctor { .. } => "doctor",
        Command::Daemon { .. } => "daemon",
        Command::Mcp { .. } => "mcp",
        Command::Open { .. } => "open",
        Command::Notes { .. } => "notes",
        Command::Sub { .. } => "sub",
        Command::Bundle { .. } => "bundle",
        Command::Workspace { .. } => "workspace",
        Command::Skill { .. } => "skill",
        Command::Prune { .. } => "prune",
    }
}

/// Daemon mode: history commands talk to the daemon directly; every todo.sh command runs against a
/// scratch copy and its diff is sent as mutations (`daemon_mode`). `env` reports the real paths.
#[tracing::instrument(
    name = "cli.command",
    skip_all,
    fields(name = command_name(command), mode = "daemon")
)]
fn dispatch_daemon(
    ctx: &Ctx,
    daemon: &mut client::Daemon,
    command: &Command,
) -> Result<(), CliError> {
    dispatch_daemon_inner(ctx, daemon, command)
}

/// The actual daemon-mode dispatch, split out of `dispatch_daemon` so `#[instrument]` (which costs
/// cognitive-complexity points on its own) never pushes this already-branchy match over the budget
/// (root todo.txt `logging-cli`).
fn dispatch_daemon_inner(
    ctx: &Ctx,
    daemon: &mut client::Daemon,
    command: &Command,
) -> Result<(), CliError> {
    match command {
        Command::Env => dispatch(ctx, command),
        Command::Log { file, n } => {
            commands::history::run_log(daemon, file.as_deref(), *n, ctx.json)
        }
        Command::Blame { item } => commands::history::run_blame(ctx, daemon, item),
        Command::Undo { steps } => commands::history::run_undo(ctx, daemon, *steps),
        Command::Checkout { at, stdout, file } => {
            let file = file.as_deref().unwrap_or(&ctx.paths.todo_file);
            commands::history::run_checkout(daemon, at, file, *stdout)
        }
        Command::Conflicts { action } => {
            commands::conflicts::run(ctx, daemon, action.as_ref(), ctx.json)
        }
        Command::Pair { code } => commands::pair::run(ctx, daemon, code.as_deref()),
        Command::Open { item } => commands::refdir::run_open(ctx, daemon, item),
        Command::Notes { item } => commands::refdir::run_notes(ctx, daemon, item),
        Command::Sub { item, cmd } => commands::refdir::run_sub(ctx, daemon, item, cmd),
        Command::Prune { orphans, yes } => {
            commands::refdir::run_prune(daemon, *orphans, *yes, ctx.json)
        }
        Command::Device { action } => commands::device::run(daemon, action.as_ref(), ctx.json),
        Command::Identity { action } => commands::identity::run(daemon, action, ctx.json),
        Command::Workspace { action } => {
            commands::workspace::run(ctx, daemon, action.as_ref(), ctx.json)
        }
        Command::Bundle { action } => bundle::run(daemon, action),
        // Every todo.sh command, present and future, goes through the scratch adapter by design.
        todo_sh => daemon_mode::run_via_daemon(ctx, daemon, |scratch| dispatch(scratch, todo_sh)),
    }
}

/// Direct-file mode (M2): one arm per command.
#[tracing::instrument(
    name = "cli.command",
    skip_all,
    fields(name = command_name(command), mode = "direct")
)]
fn dispatch(ctx: &Ctx, command: &Command) -> Result<(), CliError> {
    dispatch_inner(ctx, command)
}

/// The actual direct-mode dispatch, split out of `dispatch` for the same cognitive-complexity
/// reason as `dispatch_daemon_inner` above.
fn dispatch_inner(ctx: &Ctx, command: &Command) -> Result<(), CliError> {
    match command {
        Command::Doctor { .. }
        | Command::Daemon { .. }
        | Command::Mcp { .. }
        | Command::Skill { .. } => unreachable!("handled before mode selection"),
        c if daemon_ensure::needs_daemon(c) => Err(needs_daemon_err()),
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
        // Unreachable in practice: every variant is handled by an arm above, either the
        // `needs_daemon` guard or an explicit one — a guard is not itself proof of exhaustiveness
        // to the compiler, so this wildcard exists only to satisfy it.
        _ => unreachable!("every Command variant is handled by an arm above"),
    }
}

/// Shared by every todo.sh command listed as daemon-only in `dispatch_inner`.
fn needs_daemon_err() -> CliError {
    CliError::Message(format!("txtodo: {}", commands::history::NEEDS_DAEMON))
}
