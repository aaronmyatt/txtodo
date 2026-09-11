//! The txtodo CLI: todo.sh-compatible commands over todo.txt (plan M2, direct-file mode).
#![forbid(unsafe_code)]
#![allow(clippy::print_stdout, clippy::print_stderr)] // the CLI is the output path (plan §0)

mod config;

use clap::{Parser, Subcommand};
use config::{Config, Env, Paths};
use std::fmt;
use std::process::ExitCode;

/// todo.sh-compatible todo.txt tool. Commands and aliases match todo.sh; line numbers are the ids.
#[derive(Debug, Parser)]
#[command(name = "txtodo", version, about)]
struct Cli {
    /// Todo directory (overrides $TXTODO_TODO_DIR and config `todo_dir`).
    #[arg(long, global = true, value_name = "DIR")]
    dir: Option<String>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Print the resolved paths and config.
    Env,
}

/// Anything that ends the run with a message on stderr and exit status 1.
#[derive(Debug)]
enum CliError {
    /// The config file exists but is unusable.
    Config(config::ConfigError),
    /// The process environment could not be read.
    Io(std::io::Error),
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CliError::Config(e) => write!(f, "{e}"),
            CliError::Io(e) => write!(f, "{e}"),
        }
    }
}

/// Everything a command needs: where the files are and what the config says.
struct Ctx {
    paths: Paths,
    config: Config,
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
    let ctx = Ctx { paths, config };
    match cli.command {
        Command::Env => print_env(&ctx),
    }
    Ok(())
}

/// `txtodo env`: one `key=value` per line.
fn print_env(ctx: &Ctx) {
    let schemes = ctx.config.url_schemes();
    let exists = if ctx.paths.config.exists() {
        ""
    } else {
        " (missing)"
    };
    println!("todo_dir={}", ctx.paths.dir.display());
    println!("todo_file={}", ctx.paths.todo.display());
    println!("done_file={}", ctx.paths.done.display());
    println!("report_file={}", ctx.paths.report.display());
    println!("config_file={}{exists}", ctx.paths.config.display());
    println!("id_tags={}", ctx.config.id_tags());
    println!("url_schemes={}", schemes.join(","));
}
