//! The `clap` argument grammar: `Cli` (global flags) and `Command` (every subcommand). Split out
//! of `main.rs` for the file budget; `main.rs` keeps `run`/`dispatch`/`dispatch_daemon`.

use crate::bundle;
use crate::commands;
use clap::{Parser, Subcommand};

/// todo.sh-compatible todo.txt tool. Commands and aliases match todo.sh; line numbers are the ids.
#[derive(Debug, Parser)]
#[command(name = "txtodo", version, about)]
pub struct Cli {
    /// Todo directory (overrides $TXTODO_TODO_DIR and config `todo_dir`).
    #[arg(long, global = true, value_name = "DIR")]
    pub dir: Option<String>,
    /// File-carrier sync folder (overrides $TXTODO_SYNC_DIR and config `sync_dir`; plan M8
    /// `sync-file-carrier`). Unset means file-carrier sync is not configured.
    #[arg(long, global = true, value_name = "DIR")]
    pub sync_dir: Option<String>,
    /// Relay URL (overrides $TXTODO_RELAY_URL and config `relay_url`; plan M8
    /// `sync-relay-enable`, ADR 0026). Unset means relay stays off — LAN-only, unchanged.
    #[arg(long, global = true, value_name = "URL")]
    pub relay: Option<String>,
    /// Emit one JSON object per line on listing commands.
    #[arg(long, global = true)]
    pub json: bool,
    /// Do not stamp `id:` on added tasks (overrides config `id_tags`).
    #[arg(long, global = true)]
    pub no_id: bool,
    /// Do not archive after `do` (todo.sh -A).
    #[arg(short = 'A', long, global = true)]
    pub no_archive: bool,
    /// Ignore a running daemon and edit the files directly (M2 behaviour).
    #[arg(long, global = true)]
    pub no_daemon: bool,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
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
    /// Move completed lines to the bottom of the file and drop blank lines.
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
        action: Option<commands::conflicts::Action>,
    },
    /// Devices paired into this workspace's sync group (daemon mode): `list` (default) or
    /// `remove <id>`.
    Device {
        #[command(subcommand)]
        action: Option<commands::device::Action>,
    },
    /// Pair with another device: no CODE starts a handshake and shows a QR/code; CODE (scanned or
    /// pasted from the other device) joins it and shows the six-word SAS to compare (daemon mode,
    /// plan M4, design §4).
    Pair {
        /// The other device's offer (from its QR or `txtodo pair`'s own printed text).
        code: Option<String>,
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
    /// Serve the Model Context Protocol surface for this workspace (design §6.1).
    Mcp {
        /// Serve over stdin/stdout.
        #[arg(long)]
        stdio: bool,
        /// Serve Streamable HTTP on 127.0.0.1:8636/mcp (or 0.0.0.0 with --lan).
        #[arg(long)]
        http: bool,
        /// With --http: bind every interface and advertise _txtodo-mcp._tcp via mDNS.
        #[arg(long)]
        lan: bool,
        /// Attached to every mutation as the agent principal; required with --lan.
        #[arg(long)]
        token: Option<String>,
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
    /// Moves the whole workspace as one file (daemon mode, plan M8, design §4.5): `export` writes
    /// it, `import` reads it back — no network involved either way (air-gapped sneakernet
    /// carrier).
    Bundle {
        #[command(subcommand)]
        action: bundle::Action,
    },
    /// Manages the device-global daemon's workspace registry (ADR 0025): `list` (default),
    /// `add [DIR]`, `remove <id>`.
    Workspace {
        #[command(subcommand)]
        action: Option<commands::workspace::Action>,
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
