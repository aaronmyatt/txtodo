//! The txtodo CLI: todo.sh-compatible commands over todo.txt (plan M2, direct-file mode).
#![forbid(unsafe_code)]
#![allow(clippy::print_stdout, clippy::print_stderr)] // the CLI is the output path (plan §0)

mod clock;
mod commands;
mod config;
mod json;
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
    /// Do not archive after `do` (todo.sh -A).
    #[arg(short = 'A', long, global = true)]
    no_archive: bool,
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
    /// Move completed lines to done.txt and drop blank lines.
    Archive,
    /// Remove a task's priority.
    #[command(visible_alias = "dp")]
    Depri {
        /// Line numbers, comma or space separated.
        #[arg(required = true, num_args = 1..)]
        items: Vec<String>,
    },
    /// Mark tasks done: `x`, today's date, priority kept as `pri:`; then archive.
    Do {
        /// Line numbers, comma or space separated.
        #[arg(required = true, num_args = 1..)]
        items: Vec<String>,
    },
    /// Print the resolved paths and config.
    Env,
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
    /// Set a task's priority, A to Z.
    #[command(visible_alias = "p")]
    Pri {
        /// Line number.
        item: String,
        /// The new priority letter.
        priority: String,
    },
    /// List the `.txt` files in the todo directory, or the tasks in FILE.
    #[command(visible_alias = "lf")]
    Listfile {
        /// A file name (looked up in the todo directory) then search terms.
        #[arg(allow_hyphen_values = true)]
        args: Vec<String>,
    },
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
    /// A todo.sh-worded failure, printed as is.
    Message(String),
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
            CliError::Message(m) => write!(f, "{m}"),
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
    /// `do` archives afterwards (todo.sh default; `-A` turns it off).
    auto_archive: bool,
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
        auto_archive: !cli.no_archive,
        today: clock::today_local(),
        paths,
        config,
        json: cli.json,
    };
    match &cli.command {
        Command::Add { text } => commands::add::run(&ctx, &text.join(" "), false),
        Command::Addm { text } => commands::add::run(&ctx, &text.join(" "), true),
        Command::Archive => commands::archive::run(&ctx),
        Command::Depri { items } => commands::edit::run_depri(&ctx, items),
        Command::Do { items } => commands::edit::run_do(&ctx, items),
        Command::Pri { item, priority } => commands::edit::run_pri(&ctx, item, priority),
        Command::Env => {
            print_env(&ctx);
            Ok(())
        }
        Command::List { terms } => commands::list::list_file(&ctx, &ctx.paths.todo, terms),
        Command::Listall { terms } => commands::list::list_all(&ctx, terms),
        Command::Listpri { args } => commands::list::list_pri(&ctx, args),
        Command::Listproj { terms } => commands::list::list_words(&ctx, '+', terms),
        Command::Listcon { terms } => commands::list::list_words(&ctx, '@', terms),
        Command::Listfile { args } => match args.split_first() {
            None => commands::list::list_txt_files(&ctx),
            Some((name, terms)) => {
                let path = commands::list::find_file(&ctx, name)?;
                commands::list::list_file(&ctx, &path, terms)
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
