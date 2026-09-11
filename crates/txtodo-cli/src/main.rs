//! The txtodo CLI: todo.sh-compatible commands over todo.txt (plan M2, direct-file mode).
#![forbid(unsafe_code)]
#![allow(clippy::print_stdout, clippy::print_stderr)] // the CLI is the output path (plan §0)

mod clock;
mod commands;
mod config;
mod store;

use clap::{Parser, Subcommand};
use config::{Config, Env, Paths};
use std::fmt;
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
    /// Print the resolved paths and config.
    Env,
}

/// Anything that ends the run with a message on stderr and exit status 1.
#[derive(Debug)]
enum CliError {
    /// The config file exists but is unusable.
    Config(config::ConfigError),
    /// The process environment or the random source could not be read.
    Io(std::io::Error),
    /// A file could not be read or written.
    Store(store::StoreError),
    /// An edit argument the core rejects.
    Edit(txtodo_core::EditError),
    /// Wrong arguments; the value is the todo.sh usage line.
    Usage(&'static str),
}

impl From<store::StoreError> for CliError {
    fn from(e: store::StoreError) -> CliError {
        CliError::Store(e)
    }
}
impl From<txtodo_core::EditError> for CliError {
    fn from(e: txtodo_core::EditError) -> CliError {
        CliError::Edit(e)
    }
}
impl From<std::io::Error> for CliError {
    fn from(e: std::io::Error) -> CliError {
        CliError::Io(e)
    }
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CliError::Config(e) => write!(f, "{e}"),
            CliError::Io(e) => write!(f, "{e}"),
            CliError::Store(e) => write!(f, "{e}"),
            CliError::Edit(e) => write!(f, "{e}"),
            CliError::Usage(u) => write!(f, "usage: txtodo {u}"),
        }
    }
}

/// Everything a command needs: where the files are and what the config says.
struct Ctx {
    paths: Paths,
    config: Config,
    json: bool,
    /// Stamp `id:` on add (config `id_tags` and not `--no-id`).
    ids: bool,
    /// The local calendar date at startup.
    today: Date,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(&cli) {
        Ok(()) => ExitCode::SUCCESS,
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
        today: clock::today_local(),
        paths,
        config,
        json: cli.json,
    };
    match &cli.command {
        Command::Add { text } => commands::add::run(&ctx, &text.join(" "), false),
        Command::Addm { text } => commands::add::run(&ctx, &text.join(" "), true),
        Command::Env => {
            print_env(&ctx);
            Ok(())
        }
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
            json_str(&ctx.paths.dir.to_string_lossy()),
            json_str(&ctx.paths.todo.to_string_lossy()),
            json_str(&ctx.paths.done.to_string_lossy()),
            json_str(&ctx.paths.report.to_string_lossy()),
            json_str(&ctx.paths.config.to_string_lossy()),
            exists.is_empty(),
            ctx.config.id_tags(),
            schemes
                .iter()
                .map(|s| json_str(s))
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

/// A JSON string literal (RFC 8259 §7): quotes, backslashes and control characters escaped.
fn json_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    debug_assert!(out.len() >= s.len() + 2, "quotes added");
    debug_assert!(!out[1..out.len() - 1].contains('\n'), "newlines escaped");
    out
}
