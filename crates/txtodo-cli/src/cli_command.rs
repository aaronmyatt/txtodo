//! The `Command` subcommand enum. Split out of `main.rs` purely to keep that file within its line
//! budget as commands grow; `main.rs`'s `dispatch`/`dispatch_daemon` still match over it directly.

use clap::Subcommand;

#[derive(Debug, Subcommand)]
pub(crate) enum Command {
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
    /// Move every completed line to the bottom of the file and drop blank lines: the explicit
    /// full sort. `do` alone only moves the line it completed.
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
    /// Mark tasks done: `x`, today's date, priority kept as `pri:`; then move them to the end of
    /// the file (`-A` leaves them in place). Other done lines and blank lines are not touched.
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
    /// List every task in todo.txt, done tasks included.
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
        /// Only this document (workspace-relative), e.g. q4/todo.txt.
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
        action: Option<crate::commands::conflicts::Action>,
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
        action: crate::commands::service::Action,
        /// Overwrite an existing service file on install.
        #[arg(long)]
        force: bool,
    },
    /// Prints the resolved `ref:` directory for a line; never creates it (daemon mode, rules 2, 9).
    Open {
        /// Line number in todo.txt.
        item: String,
    },
    /// Opens `$EDITOR` on a line's `ref:`/notes.md, creating the directory lazily (daemon mode,
    /// rule 4).
    Notes {
        /// Line number in todo.txt.
        item: String,
    },
    /// Runs COMMAND with its directory scoped to a line's `ref:` sub-list (daemon mode, rule 12);
    /// a scoped `todo.sh -d`.
    Sub {
        /// Line number in todo.txt.
        item: String,
        /// The command (and its own arguments) to run inside that directory.
        #[arg(required = true, trailing_var_arg = true, allow_hyphen_values = true)]
        cmd: Vec<String>,
    },
    /// Lists `ref:` directories no line points to; deletes them only with `--yes` (daemon mode,
    /// rule 10).
    Prune {
        /// The only supported prune mode today.
        #[arg(long)]
        orphans: bool,
        /// Actually delete what would otherwise only be listed.
        #[arg(long)]
        yes: bool,
    },
}
